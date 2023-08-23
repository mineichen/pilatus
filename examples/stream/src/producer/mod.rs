use std::marker::PhantomData;

use coinbase_pro_rs::{structs::wsfeed::*, WSFeed};
use futures::channel::mpsc::Receiver;
use futures::{channel::mpsc, future::Either, stream::FusedStream, SinkExt, StreamExt};
use minfac::{Registered, ServiceCollection};
use pilatus::{
    device::{
        ActorError, ActorMessage, ActorResult, ActorSystem, DeviceContext, DeviceResult,
        DeviceValidationContext,
    },
    prelude::*,
    UpdateParamsMessage, UpdateParamsMessageError,
};
use sender_collection::SenderCollection;
use serde::{Deserialize, Serialize};
use tracing::debug;
use tracing::error;
use tracing::warn;

use self::queryable::Queryable;

mod queryable;
mod sender_collection;

pub use queryable::Heartbeat;

pub const DEVICE_TYPE: &str = "coinbase_producer";

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<Registered<ActorSystem>>()
        .register_device(DEVICE_TYPE, validator, device);
}

type Registrar = Box<dyn FnOnce(&mut SenderCollection) -> NewTopicRegistrationState + Send>;

struct DeviceState {
    topic_subscribe: mpsc::Sender<Registrar>,
    params: Params,
}

async fn validator(ctx: DeviceValidationContext<'_>) -> Result<Params, UpdateParamsMessageError> {
    ctx.params_as::<Params>()
}

enum NewTopicRegistrationState {
    Created,
    ExistedAlready,
}

enum HandleResult {
    Ok,
    RequireStreamRestart,
    StopStreaming,
}

async fn device(ctx: DeviceContext, params: Params, actor_system: ActorSystem) -> DeviceResult {
    let id = ctx.id;
    let (tx, recv_subscription) = mpsc::channel(10);

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
        stream_binance_data(recv_subscription),
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

impl<T: Send + Sync + 'static> ActorMessage for SubscribeMessage<T> {
    type Output = mpsc::Receiver<T>;
    type Error = anyhow::Error;
}

impl DeviceState {
    async fn subscribe<T: Queryable + Send + Sync + 'static>(
        &mut self,
        m: SubscribeMessage<T>,
    ) -> ActorResult<SubscribeMessage<T::Response>> {
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
        UpdateParamsMessage { params }: UpdateParamsMessage<Params>,
    ) -> ActorResult<UpdateParamsMessage<Params>> {
        self.params = params;
        Ok(())
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Params {
    initial_count: u32,
}

pub fn create_default_device_config() -> pilatus::DeviceConfig {
    pilatus::DeviceConfig::new_unchecked(DEVICE_TYPE, DEVICE_TYPE, Params::default())
}

async fn stream_binance_data(mut recv_subscription: Receiver<Registrar>) {
    //let mut heartbeats: HashMap<String, Vec<mpsc::Sender<Heartbeat>>> = Default::default();
    let mut col = SenderCollection::default();
    'wait_first_topic: while let Some(registrar) = recv_subscription.next().await {
        (registrar)(&mut col);
        debug!("Start: {col:?}");

        'restart_stream: loop {
            debug!("Reconnect to coinbase.");
            let Ok(mut coinbase_stream) = WSFeed::connect(
                "wss://ws-feed.pro.coinbase.com",
                col.get_active_topics().as_slice(),
                &[ChannelType::Heartbeat],
            )
            .await else {
                warn!("Cannot connect: Wait 1s before retry");
                if recv_subscription.is_terminated() {
                    return;
                } else {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            loop {
                match futures::future::select(recv_subscription.next(), coinbase_stream.next())
                    .await
                {
                    Either::Left((Some(query), _)) => match (query)(&mut col) {
                        NewTopicRegistrationState::Created => {
                            coinbase_stream.close().await.ok();
                            debug!("Restart: {col:?}");
                            break;
                        }
                        NewTopicRegistrationState::ExistedAlready => {
                            debug!("Restart not required: {col:?}");
                            continue;
                        }
                    },
                    Either::Left((None, _)) => {
                        coinbase_stream.close().await.ok();
                        debug!("ActorSystem is down, so the connection to coinbase is dropped");
                        return;
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
                        continue 'restart_stream;
                    }
                    Either::Right((None, _)) => {
                        warn!("Coinbase stream closed unexpectedly. Reconnecting");
                        continue 'restart_stream;
                    }
                }
            }
        }
    }
}
