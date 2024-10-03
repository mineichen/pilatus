use std::{fmt::Debug, marker::PhantomData, num::Saturating};

use futures::{stream::BoxStream, StreamExt};
use serde::{Deserialize, Serialize};
use stream_broadcast::StreamBroadcast;
use tracing::{error, trace};

use crate::device::{
    ActorError, ActorMessage, ActorResult, ActorSystem, DeviceContext, DeviceId,
    WeakUntypedActorMessageSender,
};

pub struct SubscribeMessage<Q, T, E> {
    pub query: Q,
    phantom: PhantomData<(T, E)>,
}

impl<Q: Send + 'static, T: Send + 'static, E: Send + Debug + 'static> ActorMessage
    for SubscribeMessage<Q, T, E>
{
    type Output = BoxStream<'static, T>;
    type Error = E;
}

impl<Q, T, E> From<Q> for SubscribeMessage<Q, T, E> {
    fn from(query: Q) -> Self {
        Self {
            query,
            phantom: Default::default(),
        }
    }
}

impl<Q: Default, T, E> Default for SubscribeMessage<Q, T, E> {
    fn default() -> Self {
        Self {
            query: Default::default(),
            phantom: Default::default(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[non_exhaustive]
pub struct SubscribeParams {
    pub provider: DeviceId,
}

impl SubscribeParams {
    pub fn with_provider(provider: DeviceId) -> Self {
        Self { provider }
    }
}

#[derive(Debug, thiserror::Error, Clone)]
#[error("Missed {number} items")]
#[non_exhaustive]
pub struct MissedItemsError {
    pub number: Saturating<u16>,
}

impl MissedItemsError {
    pub fn new(number: Saturating<u16>) -> Self {
        Self { number }
    }
}

pub struct SubscribeState<TResult> {
    params: SubscribeParams,
    actor_system: ActorSystem,
    self_sender: WeakUntypedActorMessageSender,
    pipeline: Box<dyn Fn() -> Option<BoxStream<'static, TResult>> + Send>,
}

impl<T> SubscribeState<T> {
    pub fn new(ctx: &DeviceContext, actor_system: ActorSystem, params: SubscribeParams) -> Self {
        let self_sender = actor_system
            .get_weak_untyped_sender(ctx.id)
            .expect("Must be alive");
        Self {
            params,
            actor_system,
            self_sender,
            pipeline: Box::new(|| None),
        }
    }
    pub fn update_params(&mut self, params: SubscribeParams) {
        self.params = params;
    }
}

impl<TOutput: Send + 'static, EOutput: Send + Debug + 'static>
    SubscribeState<Result<TOutput, EOutput>>
{
    pub async fn subscribe<
        Q: Send + 'static,
        TProcessMsg: ActorMessage<Output = TOutput> + From<TOutput>,
    >(
        as_ref_state: &mut impl AsMut<SubscribeState<Result<TOutput, EOutput>>>,
        msg: SubscribeMessage<Q, Result<TProcessMsg::Output, EOutput>, ()>,
    ) -> ActorResult<SubscribeMessage<Q, Result<TProcessMsg::Output, EOutput>, ()>>
    where
        TOutput: Clone,
        EOutput: Clone + From<ActorError<TProcessMsg::Error>> + From<MissedItemsError>,
    {
        Self::subscribe_with_input::<Q, TProcessMsg, TOutput>(as_ref_state, msg).await
    }

    pub async fn subscribe_with_input<
        Q: Send + 'static,
        TProcessMsg: ActorMessage<Output = TOutput> + From<TInput>,
        TInput: Send + 'static,
    >(
        as_ref_state: &mut impl AsMut<SubscribeState<Result<TOutput, EOutput>>>,
        msg: SubscribeMessage<Q, Result<TProcessMsg::Output, EOutput>, ()>,
    ) -> ActorResult<SubscribeMessage<Q, Result<TProcessMsg::Output, EOutput>, ()>>
    where
        TOutput: Clone,
        EOutput: Clone + From<ActorError<TProcessMsg::Error>> + From<MissedItemsError>,
    {
        let this = as_ref_state.as_mut();
        if let Some(x) = (this.pipeline)() {
            return Ok(x);
        }
        let self_sender = this.self_sender.clone();
        let provider = this.params.provider;
        let inner = this
            .actor_system
            .ask(
                provider,
                SubscribeMessage::<Q, Result<TInput, EOutput>, ()>::from(msg.query),
            )
            .await?
            .then(move |r| {
                let mut actor_system = self_sender.clone();
                async move {
                    match r {
                        Ok(data) => {
                            let time = std::time::Instant::now();

                            match actor_system.ask(TProcessMsg::from(data)).await {
                                Ok(x) => {
                                    trace!("Processed Pointcloud in {:?}", time.elapsed());
                                    Ok(x)
                                }
                                Err(e) => {
                                    error!("Error processing pc: {e}");
                                    Err(EOutput::from(e))
                                }
                            }
                        }
                        Err(e) => Err(EOutput::from(e)),
                    }
                }
            });

        let stream = StreamBroadcast::new(inner.fuse(), 2);
        let downgraded = stream.downgrade();
        this.pipeline = Box::new(move || {
            downgraded.re_subscribe().upgrade().map(|x| {
                Box::pin(x.flat_map(|(missed, data)| {
                    futures::stream::iter((missed > 0).then(|| {
                        Err(MissedItemsError::new(std::num::Saturating(
                            missed.min(u16::MAX as u64) as u16,
                        ))
                        .into())
                    }))
                    .chain(futures::stream::once(std::future::ready(data)))
                })) as _
            })
        });
        Ok(Box::pin(stream.flat_map(|(missed, data)| {
            futures::stream::iter((missed > 0).then(|| {
                Err(
                    MissedItemsError::new(std::num::Saturating(missed.min(u16::MAX as u64) as u16))
                        .into(),
                )
            }))
            .chain(futures::stream::once(std::future::ready(data)))
        })) as _)
    }
}
