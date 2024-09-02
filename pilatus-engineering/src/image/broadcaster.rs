//! Use the Device-Event-Queue to schedule broadcast of images

use std::{fmt::Debug, marker::PhantomData, sync::Arc};

use futures::{future::BoxFuture, stream::BoxStream, StreamExt};
use pilatus::device::{
    ActorDevice, ActorError, ActorMessage, ActorResult, ActorWeakTellError,
    WeakUntypedActorMessageSender,
};
use tokio::sync::broadcast;
use tracing::{debug, trace, warn};

use crate::image::{BroadcastImage, GetImageOk, SubscribeImageMessage};

pub struct BroadcastState<TError: Debug, TState> {
    // Option is used instead of receiver_count(). The later could have lead to concurrency issues
    transmitter: Option<broadcast::Sender<BroadcastImage>>,
    event_publisher: WeakUntypedActorMessageSender,
    async_producer: BroadcastProducer<TState, TError>,
    stop_broadcast_callback: fn(&mut TState),
}

pub trait RegisterBroadcastHandlersExtension<TError> {
    type Error;
    fn add_broadcast_handlers(self) -> Self;
}

impl<
        TError: Debug + Send + Sync + 'static,
        TState: AsMut<BroadcastState<TError, TState>> + Send + Sync + 'static,
    > RegisterBroadcastHandlersExtension<TError> for ActorDevice<TState>
{
    type Error = TError;

    fn add_broadcast_handlers(self) -> Self {
        async fn broadcast_image<
            TError: Debug + Send + Sync + 'static,
            TState: AsMut<BroadcastState<TError, TState>>,
        >(
            state: &mut TState,
            _msg: BroadcastImageMessage<TError>,
        ) -> Result<(), ActorError<TError>> {
            if state.as_mut().transmitter.is_some() {
                let producer_copy = state.as_mut().async_producer;

                match (producer_copy)(state).await {
                    Ok(output) => {
                        let this = state.as_mut();
                        let broadcaster = this.transmitter.as_mut().expect(
                            "Checked before. async_producer mustn't remove the transmitter!",
                        );
                        if broadcaster
                            .send(BroadcastImage {
                                image: Arc::new(output.image),
                                hash: output.meta.hash,
                            })
                            .is_ok()
                        {
                            this.event_publisher
                                .tell(BroadcastImageMessage::<TError>(PhantomData))?;
                            return Ok(());
                        }
                    }
                    Err(e) => {
                        warn!(msg = %e, "broadcast error");
                    }
                }

                let this = state.as_mut();
                let callback = this.stop_broadcast_callback;
                this.transmitter = None;
                (callback)(state)
            }

            Ok(())
        }

        async fn subscribe_broadcast_image<
            TError: Debug + Send + Sync + 'static,
            TState: AsMut<BroadcastState<TError, TState>> + Send + Sync + 'static,
        >(
            state: &mut TState,
            _: SubscribeImageMessage,
        ) -> ActorResult<SubscribeImageMessage> {
            debug!("Subscribe broadcast");
            Ok(state.as_mut().subscribe()?)
        }

        self.add_handler(broadcast_image::<TError, TState>)
            .add_handler(subscribe_broadcast_image::<TError, TState>)
    }
}

type BroadcastProducer<TState, TError> =
    for<'a> fn(&'a mut TState) -> BoxFuture<'a, Result<GetImageOk, ActorError<TError>>>;

impl<
        TError: Debug + Send + Sync + 'static,
        TState: AsMut<BroadcastState<TError, TState>> + Send + Sync + 'static,
    > BroadcastState<TError, TState>
{
    pub fn is_streaming(&self) -> bool {
        self.transmitter.is_some()
    }

    pub fn new(
        event_publisher: WeakUntypedActorMessageSender,
        async_producer: BroadcastProducer<TState, TError>,
        stop_broadcast_callback: fn(&mut TState),
    ) -> Self {
        Self {
            transmitter: None,
            event_publisher,
            async_producer,
            stop_broadcast_callback,
        }
    }
    fn subscribe(&mut self) -> Result<BoxStream<'static, BroadcastImage>, ActorWeakTellError> {
        Ok(
            tokio_stream::wrappers::BroadcastStream::new(match &mut self.transmitter {
                Some(x) => x.subscribe(),
                None => {
                    let (tx, rx) = broadcast::channel(1);
                    self.transmitter = Some(tx);
                    self.event_publisher
                        .tell(BroadcastImageMessage::<TError>(PhantomData))?;
                    rx
                }
            })
            .filter_map(|x| async {
                trace!("Lost image");
                x.ok()
            })
            .boxed(),
        )
    }
}

// This message get's reattached to the main MessageQueue if broadcast was successful.
// This allows other messages like ConfigurationChanges, Subscriptions... to sneak in between
struct BroadcastImageMessage<TError: Send + Sync + 'static>(PhantomData<TError>);

impl<TError: Debug + Send + Sync + 'static> ActorMessage for BroadcastImageMessage<TError> {
    type Output = ();
    type Error = TError;
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicU8, Ordering},
            Arc,
        },
        time::Duration,
    };

    use futures::FutureExt;
    use tracing::debug;

    use pilatus::device::{ActorError, ActorSystem, DeviceId};

    use super::*;

    #[tokio::test]
    async fn test_subscribe_after_camera_failure() {
        struct ActorState {
            create_image_counter: Arc<AtomicU8>,
            broadcast: BroadcastState<(), ActorState>,
        }

        impl AsMut<BroadcastState<(), ActorState>> for ActorState {
            fn as_mut(&mut self) -> &mut BroadcastState<(), ActorState> {
                &mut self.broadcast
            }
        }
        let counter = Arc::<std::sync::atomic::AtomicU8>::default();
        let actor_system = Arc::new(ActorSystem::new());
        let runner_actor_system = actor_system.clone();
        let runner_counter = counter.clone();
        let id = DeviceId::new_v4();
        let runner = runner_actor_system.register(id);
        let mut runner = Box::pin(async move {
            let state = ActorState {
                create_image_counter: runner_counter,
                broadcast: BroadcastState::new(
                    runner_actor_system.get_weak_untyped_sender(id).unwrap(),
                    |c: &mut ActorState| {
                        async {
                            let old_value = c.create_image_counter.load(Ordering::SeqCst);
                            c.create_image_counter
                                .store(old_value + 1, Ordering::SeqCst);
                            Err(ActorError::Custom(()))
                        }
                        .boxed()
                    },
                    |_| debug!("Unsubscribe from Simulation-Camera"),
                ),
            };
            runner.add_broadcast_handlers().execute(state).await;
        });
        tokio::select! {
            _ = &mut runner => {
                panic!("Shouldn't finish");
            }
            _ = async{
                let _s = actor_system.ask(id, SubscribeImageMessage {}).await.expect("Should accept subscription");
            } => {}
        };
        tokio::select! {
            _ = &mut runner => {
                panic!("Shouldn't finish");
            }
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
        };
        tokio::select! {
            _ = &mut runner => {
                panic!("Shouldn't finish");
            }
            _ = async{
                let _s = actor_system.ask(id, SubscribeImageMessage {}).await.expect("Should accept subscription");
            } => {}
        };
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
