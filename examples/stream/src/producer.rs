use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::marker::PhantomData;

use coinbase_pro_rs::structs::wsfeed::*;
use coinbase_pro_rs::structs::DateTime;
use coinbase_pro_rs::WSFeed;
use futures::future::Either;
use futures::SinkExt;
use futures::{channel::mpsc, StreamExt};
use minfac::{Registered, ServiceCollection};
use pilatus::device::ActorError;
use pilatus::{
    device::{
        ActorMessage, ActorResult, ActorSystem, DeviceContext, DeviceResult,
        DeviceValidationContext,
    },
    prelude::*,
    UpdateParamsMessage, UpdateParamsMessageError,
};
use serde::{Deserialize, Serialize};
use tracing::debug;
use tracing::error;
use tracing::warn;

pub const DEVICE_TYPE: &str = "producer";

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<Registered<ActorSystem>>()
        .register_device(DEVICE_TYPE, validator, device);
}

struct DeviceState {
    topic_subscribe: mpsc::Sender<Box<dyn FnOnce(&mut SenderCollection) -> TopicState + Send>>,
    params: CoinbaseParams,
}

async fn validator(
    ctx: DeviceValidationContext<'_>,
) -> Result<CoinbaseParams, UpdateParamsMessageError> {
    ctx.params_as_sealed::<CoinbaseParamsRaw>()
}

enum TopicState {
    Created,
    ExistedAlready,
}

trait Queryable: Sized {
    const CHANNEL_TYPE: ChannelType;
    fn create_fn(
        topic: String,
        x: mpsc::Sender<Self>,
    ) -> Box<dyn FnOnce(&mut SenderCollection) -> TopicState + Send>;
}

#[derive(Debug, Serialize)]
pub struct Heartbeat {
    sequence: u64,
    time: DateTime,
    last_trade_id: u64,
}

impl Queryable for Heartbeat {
    const CHANNEL_TYPE: ChannelType = ChannelType::Heartbeat;
    fn create_fn(
        topic: String,
        sender: mpsc::Sender<Self>,
    ) -> Box<dyn FnOnce(&mut SenderCollection) -> TopicState + Send> {
        Box::new(move |c| match c.heartbeat.entry(topic) {
            Entry::Occupied(mut x) => {
                x.get_mut().push(sender);
                TopicState::ExistedAlready
            }
            Entry::Vacant(x) => {
                x.insert(vec![sender]);
                TopicState::Created
            }
        })
    }
}

#[derive(Default)]
struct SenderCollection {
    heartbeat: HashMap<String, Vec<mpsc::Sender<Heartbeat>>>,
    full: HashMap<String, Vec<mpsc::Sender<Full>>>,
}

impl std::fmt::Debug for SenderCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SenderCollection")
            .field("heartbeat", &self.heartbeat.len())
            .field("full", &self.full.len())
            .finish()
    }
}

enum HandleResult {
    Ok,
    RequireStreamRestart,
    StopStreaming,
}

impl SenderCollection {
    fn get_active_topics(&self) -> Vec<&str> {
        self.heartbeat
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
    }
    fn is_empty(&self) -> bool {
        self.heartbeat.is_empty() && self.full.is_empty()
    }
    fn handle_message(&mut self, msg: Message) -> HandleResult {
        match msg {
            Message::Heartbeat {
                sequence,
                last_trade_id,
                product_id,
                time,
            } => {
                let Some(topic) = self.heartbeat.get_mut(&product_id) else {
                    error!("Websocket sent broadcast for product_id noone is listening to");
                    return HandleResult::Ok;
                };
                let prev_len = topic.len();
                topic.retain_mut(|v| {
                    v.try_send(Heartbeat {
                        sequence: sequence as _,
                        time,
                        last_trade_id: last_trade_id as _,
                    })
                    .is_ok()
                });
                debug!(
                    "Listeners on topic {product_id}, Prev: {prev_len}, now: {}",
                    topic.len()
                );
                if topic.is_empty() {
                    self.heartbeat
                        .remove(&product_id)
                        .expect("Must have been there");
                    if self.is_empty() {
                        debug!("All listeners exited. Stream to coinbase closes");
                        HandleResult::StopStreaming
                    } else {
                        HandleResult::RequireStreamRestart
                    }
                } else {
                    HandleResult::Ok
                }
            }

            m => {
                warn!("Unknown message {m:?} is ignored");
                HandleResult::Ok
            }
        }
    }
}

async fn device(
    ctx: DeviceContext,
    params: CoinbaseParams,
    actor_system: ActorSystem,
) -> DeviceResult {
    let id = ctx.id;
    let (tx, mut recv_subscription) = mpsc::channel(10);

    futures::future::join(
        async {
            actor_system
                .register(id)
                .add_handler(DeviceState::subscribe::<Heartbeat>)
                .add_handler(DeviceState::update_params)
                .execute(&mut DeviceState {
                    topic_subscribe: tx,
                    params,
                })
                .await;
        },
        async move {
            //let mut heartbeats: HashMap<String, Vec<mpsc::Sender<Heartbeat>>> = Default::default();
            let mut col = SenderCollection::default();
            'wait_first_topic: while let Some(registrar) = recv_subscription.next().await {
                (registrar)(&mut col);
                debug!("Start: {col:?}");

                'restart_stream: loop {
                    debug!("Reconnect to coinbase.");
                    let mut coinbase_stream = match WSFeed::connect(
                        "wss://ws-feed.pro.coinbase.com",
                        col.get_active_topics().as_slice(),
                        &[ChannelType::Heartbeat],
                    )
                    .await
                    {
                        Ok(x) => x,
                        Err(_) => {
                            // Trace: Cannot connect
                            warn!("Cannot connect: Wait 1s before retry");
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            continue;
                        }
                    };

                    loop {
                        match futures::future::select(recv_subscription.next(), coinbase_stream.next()).await
                        {
                            Either::Left((subscription, _)) => {
                                debug!("Additional subscription dropped in");
                                if let Some(query) = subscription {
                                    match(query)(&mut col) {
                                        TopicState::Created => {
                                            coinbase_stream.close().await.ok();
                                            debug!("Restart: {col:?}");
                                            break
                                        },
                                        TopicState::ExistedAlready => {
                                            debug!("Restart not required: {col:?}");
                                            continue;
                                        }
                                    }
                                } else {
                                    coinbase_stream.close().await.ok();
                                    debug!("ActorSystem is down, so the connection to coinbase is dropped");
                                    return;
                                }
                            }
                            Either::Right((Some(Ok(msg)), _)) => match col.handle_message(msg) {
                                HandleResult::Ok => {}
                                HandleResult::RequireStreamRestart => {
                                    coinbase_stream.close().await.ok();
                                    continue 'restart_stream;
                                }
                                HandleResult::StopStreaming => {
                                    coinbase_stream.close().await.ok();
                                    continue 'wait_first_topic;
                                }
                            },
                            Either::Right((Some(Err(e)), _)) => {
                                error!("Coinbase stream caused error: {e}. Reconnecting");
                                continue 'restart_stream
                            },
                            Either::Right((None, _)) => {
                                warn!("Coinbase stream closed unexpectedly. Reconnecting");
                                continue 'restart_stream
                            }
                        }
                    }
                }
            }
        },
    )
    .await;

    Ok(())
}

pub struct SubscribeMessage<T> {
    product_id: String,
    channel: PhantomData<T>,
}

impl<T> SubscribeMessage<T> {
    pub fn new(x: impl Into<String>) -> Self {
        Self {
            product_id: x.into(),
            channel: PhantomData,
        }
    }
}

impl<T: Queryable + Send + Sync + 'static> ActorMessage for SubscribeMessage<T> {
    type Output = mpsc::Receiver<T>;
    type Error = anyhow::Error;
}

impl DeviceState {
    async fn subscribe<T: Queryable + Send + Sync + 'static>(
        &mut self,
        m: SubscribeMessage<T>,
    ) -> ActorResult<SubscribeMessage<T>> {
        let (tx, rx) = mpsc::channel(10);
        self.topic_subscribe
            .send(T::create_fn(m.product_id, tx))
            .await
            .map_err(|e| {
                ActorError::custom(anyhow::Error::from(e).context("Too many requests pending"))
            })?;
        debug!("Subscription sent to async task");
        Ok(rx)
    }
    async fn update_params(
        &mut self,
        UpdateParamsMessage { params }: UpdateParamsMessage<CoinbaseParams>,
    ) -> ActorResult<UpdateParamsMessage<CoinbaseParams>> {
        self.params = params;
        Ok(())
    }
}

#[derive(
    Debug, Default, Deserialize, Serialize, sealedstruct::Seal, sealedstruct::TryIntoSealed,
)]
#[serde(deny_unknown_fields)]
pub struct CoinbaseParamsRaw {
    initial_count: u32,
}

pub fn create_default_device_config() -> pilatus::DeviceConfig {
    pilatus::DeviceConfig::new_with_unchecked_name(
        DEVICE_TYPE,
        DEVICE_TYPE,
        CoinbaseParamsRaw::default(),
    )
}
