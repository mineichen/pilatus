use std::{
    any::{Any, TypeId},
    borrow::Cow,
    collections::{HashMap, HashSet},
    convert::Infallible,
    fmt::Debug,
    future::poll_fn,
    marker::PhantomData,
    sync::{Arc, Mutex, RwLock},
    task::Poll,
};

use futures::{
    channel::{mpsc, oneshot},
    future::{BoxFuture, Either},
    pin_mut,
    stream::{AbortRegistration, Abortable, FuturesUnordered},
    FutureExt, StreamExt,
};
use minfac::{Registered, ServiceCollection};
use tracing::trace;

use self::sealed::ActorSystemIdentifier;

use super::DeviceId;

mod error;
mod handler_closure;
mod handler_result;
mod sender;

pub use error::*;
pub use handler_closure::*;
pub use handler_result::*;
pub use sender::*;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.register_shared::<RwLock<ActorSystemState>>(Default::default);
    c.with::<Registered<Arc<RwLock<ActorSystemState>>>>()
        .register(|state| ActorSystem { state });
}

pub trait ActorMessage: Any + Send {
    type Output: 'static + Send;
    type Error: Debug + 'static + Send;
}

pub struct BoxMessage(Box<dyn Any + Send>);

#[derive(Debug, Clone)]
pub struct ActorSystem {
    state: SharedActorSystemState,
}

impl ActorSystem {
    pub fn new() -> Self {
        Self {
            state: Default::default(),
        }
    }

    // After forgetting the senders, the system should finish pending tasks and shutdown eventually.
    // It is therefore essential that Actors dont have persistent cyclic senders.
    // If so, consider using a Weak-Sender or request the sender for each new request to avoid unstoppable recipes.
    pub fn forget_senders(&self) {
        self.state
            .write()
            .expect("Shouldnt be poisoned")
            .devices
            .clear();
    }

    #[cfg(all(feature = "unstable", feature = "tokio"))]
    pub async fn run_and_shutdown<F: std::future::Future<Output = ()> + 'static>(
        &self,
        x: impl (FnOnce(Self) -> F) + Send + Sync + 'static,
    ) {
        // Wait till devices ready
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        (x)(self.clone()).await;
        self.forget_senders();
    }

    pub fn register<TState>(&self, device_id: DeviceId) -> ActorDevice<TState> {
        let (sender, receiver) = mpsc::channel(10);
        {
            let mut lock = self.state.write().expect("Shouldnt be poisoned");
            lock.devices.insert(device_id, Arc::new(sender));
        }
        ActorDevice::new(
            receiver,
            releaser::DeviceReleaser::new(device_id, self.state.clone()),
        )
    }

    pub fn list_devices_for_message_type<TMsg: Any>(&self) -> HashSet<DeviceId> {
        let lock = self.state.read().expect("Not poisoned");
        match lock.messages.get(&TypeId::of::<TMsg>()) {
            Some(x) => x.clone(),
            None => HashSet::new(),
        }
    }
    pub fn list_devices_for_message_types(
        &self,
        types: impl IntoIterator<Item = TypeId>,
    ) -> HashSet<DeviceId> {
        let lock = self.state.read().expect("Not poisoned");
        let empty = HashSet::new();
        let mut iter = types
            .into_iter()
            .map(|type_id| lock.messages.get(&type_id).unwrap_or(&empty));

        let mut init = match iter.next() {
            Some(x) => x.clone(),
            None => return HashSet::new(),
        };

        for next in iter {
            init.retain(|i| next.contains(i))
        }

        init
    }

    pub fn get_weak_untyped_sender(
        &self,
        device_id: DeviceId,
    ) -> Result<WeakUntypedActorMessageSender, ActorErrorUnknownDevice> {
        let mpsc_sender = {
            let lock = self.state.read().expect("Should never be poisoned");

            Arc::downgrade(lock.devices.get(&device_id).ok_or(
                ActorErrorUnknownDevice::UnknownDeviceId {
                    device_id,
                    details: "Unknown Id".into(),
                },
            )?)
        };
        Ok(WeakUntypedActorMessageSender::new(device_id, mpsc_sender))
    }

    pub fn get_untyped_sender(
        &self,
        device_id: impl ActorSystemIdentifier,
    ) -> Result<UntypedActorMessageSender, ActorErrorUnknownDevice> {
        let lock = self.state.read().expect("Should never be poisoned");
        device_id.get_untyped_sender(sealed::SealedActorSystemState(&lock))
    }

    pub fn get_sender<T: ActorMessage>(
        &self,
        device_id: impl sealed::ActorSystemIdentifier,
    ) -> Result<ActorMessageSender<T>, ActorErrorUnknownDevice> {
        self.get_untyped_sender(device_id)
            .map(ActorMessageSender::new)
    }

    pub fn get_senders<TMsg: ActorMessage>(
        &self,
    ) -> impl Iterator<Item = (DeviceId, ActorMessageSender<TMsg>)> + '_ {
        self.list_devices_for_message_type::<TMsg>()
            .into_iter()
            .filter_map(|id| self.get_sender(id).ok().map(|x| (id, x)))
    }

    pub fn get_sender_or_single_handler<TMsg: ActorMessage>(
        &self,
        id: Option<DeviceId>,
    ) -> Result<ActorMessageSender<TMsg>, ActorErrorUnknownDevice> {
        match id {
            Some(id) => self.get_sender(id),
            None => {
                let ids = self.list_devices_for_message_type::<TMsg>();
                let mut ids_iter = ids.iter();
                let Some(id) = ids_iter.next() else {
                    return Err(ActorErrorUnknownDevice::AmbiguousHandler {
                        msg_type: std::any::type_name::<TMsg>(),
                        possibilities: Default::default(),
                    });
                };

                if ids_iter.next().is_none() {
                    self.get_sender(*id)
                } else {
                    Err(ActorErrorUnknownDevice::AmbiguousHandler {
                        msg_type: std::any::type_name::<TMsg>(),
                        possibilities: ids,
                    })
                }
            }
        }
    }

    pub async fn ask<TMsg: ActorMessage>(
        &self,
        device_id: DeviceId,
        msg: TMsg,
    ) -> ActorResult<TMsg> {
        self.get_untyped_sender(device_id)?.ask(msg).await
    }
}

mod sealed {
    use futures::channel::mpsc;

    use super::{ActorErrorUnknownDevice, ActorSystemState, UntypedActorMessageSender};
    use crate::device::DeviceId;

    pub struct SealedActorSystemState<'a>(pub(super) &'a ActorSystemState);

    pub trait ActorSystemIdentifier {
        fn get_untyped_sender(
            self,
            actor_system: SealedActorSystemState,
        ) -> Result<UntypedActorMessageSender, ActorErrorUnknownDevice>;
    }

    impl ActorSystemIdentifier for DeviceId {
        fn get_untyped_sender(
            self,
            state: SealedActorSystemState,
        ) -> Result<UntypedActorMessageSender, ActorErrorUnknownDevice> {
            let mpsc_sender = state
                .0
                .devices
                .get(&self)
                .map(|x| mpsc::Sender::clone(&x))
                .ok_or(ActorErrorUnknownDevice::UnknownDeviceId {
                    device_id: self,
                    details: "No message queue for this device".into(),
                })?;
            Ok(UntypedActorMessageSender::new(self, mpsc_sender))
        }
    }
}

impl Default for ActorSystem {
    fn default() -> Self {
        Self::new()
    }
}

type SharedActorSystemState = Arc<RwLock<ActorSystemState>>;
type InternalSender = mpsc::Sender<(TypeId, BoxMessage)>;

#[derive(Debug, Default)]
#[allow(clippy::type_complexity)]
struct ActorSystemState {
    devices: HashMap<DeviceId, Arc<InternalSender>>,
    /// Map from a MessageType to Uuid of Actors which are able to handle the message
    messages: HashMap<TypeId, HashSet<DeviceId>>,
}

struct MessageWithResponse<TMsg: ActorMessage> {
    msg: TMsg,
    response_channel: oneshot::Sender<ActorResult<TMsg>>,
}

impl<TMsg: ActorMessage> MessageWithResponse<TMsg> {
    fn new(msg: TMsg, response_channel: oneshot::Sender<ActorResult<TMsg>>) -> Self {
        Self {
            msg,
            response_channel,
        }
    }
}

pub trait MessageHandler<TState>: Send + Sync {
    fn handle(
        &self,
        state: TState,
        boxed_msg: BoxMessage,
    ) -> BoxFuture<'static, (TState, HandlerClosureResponse)>;
    fn respond_with_unknown_device(
        &self,
        state: &mut TState,
        boxed_msg: BoxMessage,
        detail: Cow<'static, str>,
    );
}

#[cfg(any(feature = "tokio", feature = "rayon", test))]
struct SyncMessageHandler<TState, TMsg: ActorMessage>(
    fn(&mut TState, TMsg) -> ActorResult<TMsg>,
    PhantomData<(TState, Mutex<TMsg>)>,
);

#[cfg(any(feature = "tokio", feature = "rayon", test))]
impl<TState, TMsg> MessageHandler<TState> for SyncMessageHandler<TState, TMsg>
where
    TState: Send + Sync + 'static,
    TMsg: ActorMessage,
{
    fn handle(
        &self,
        mut state: TState,
        boxed_msg: BoxMessage,
    ) -> BoxFuture<'static, (TState, HandlerClosureResponse)> {
        let MessageWithResponse {
            msg,
            response_channel,
        } = *boxed_msg
            .0
            .downcast::<MessageWithResponse<TMsg>>()
            .expect("Must be castable. This is most likely an internal bug of the ActorSystem");
        let func = self.0;

        async move {
            let (r, state) =
                crate::sync::process_blocking::<_, ActorError<Infallible>>(move || {
                    let r = (func)(&mut state, msg);
                    Ok((r, state))
                })
                .await
                .expect("Can't fail here, otherwise state is gone");
            response_channel.send(r).ok();

            (state, None)
        }
        .boxed()
    }

    fn respond_with_unknown_device(
        &self,
        _state: &mut TState,
        boxed_msg: BoxMessage,
        detail: Cow<'static, str>,
    ) {
        respond_with_unknown_device::<TMsg>(boxed_msg, detail)
    }
}

struct AsyncMessageHandler<THandlerClosure: Send + Sync, TState, TMsg> {
    closure: THandlerClosure,
    phantom: PhantomData<(TState, Mutex<TMsg>)>,
}

impl<THandlerClosure, TState, TMsg> MessageHandler<TState>
    for AsyncMessageHandler<THandlerClosure, TState, TMsg>
where
    THandlerClosure: for<'a> AsyncHandlerClosure<'a, TState, TMsg> + 'static + Send + Sync + Clone,
    TState: Send + Sync + 'static,
    TMsg: ActorMessage,
{
    fn handle(
        &self,
        mut state: TState,
        boxed_msg: BoxMessage,
    ) -> BoxFuture<'static, (TState, HandlerClosureResponse)> {
        let h_cloned = self.closure.clone();
        let h_cloned: THandlerClosure = h_cloned;
        let MessageWithResponse {
            msg,
            response_channel,
        } = *boxed_msg
            .0
            .downcast::<MessageWithResponse<TMsg>>()
            .expect("Must be castable. This is most likely an internal bug of the ActorSystem");

        trace!(
            "Received Message of type '{:?}'",
            std::any::type_name::<TMsg>()
        );
        async move {
            let r = h_cloned
                .call(&mut state, msg, HandlerClosureContext { response_channel })
                .await;
            (state, r)
        }
        .boxed()
    }

    fn respond_with_unknown_device(
        &self,
        _state: &mut TState,
        boxed_msg: BoxMessage,
        detail: Cow<'static, str>,
    ) {
        respond_with_unknown_device::<TMsg>(boxed_msg, detail)
    }
}

fn respond_with_unknown_device<TMsg: ActorMessage>(
    boxed_msg: BoxMessage,
    details: Cow<'static, str>,
) {
    let MessageWithResponse {
        response_channel, ..
    } = *boxed_msg
        .0
        .downcast::<MessageWithResponse<TMsg>>()
        .expect("Must be castable. This is most likely an internal bug of the ActorSystem");
    let _ignore_not_consumed =
        response_channel.send(Err(ActorErrorUnknownDevice::UnknownDeviceId {
            device_id: DeviceId::nil(),
            details,
        }
        .into()));
}

mod releaser {
    use std::any::TypeId;

    use super::SharedActorSystemState;
    use crate::device::DeviceId;

    pub(super) struct DeviceReleaser {
        id: DeviceId,
        pub state: super::SharedActorSystemState,
    }

    impl DeviceReleaser {
        pub fn new(id: DeviceId, state: SharedActorSystemState) -> Self {
            Self { id, state }
        }

        pub fn publish_message(&self, typeid: TypeId) {
            let mut lock = self.state.write().expect("Not poisoned");
            lock.messages.entry(typeid).or_default().insert(self.id);
        }

        pub fn revoke_message_responsibility(&self, typeids: impl IntoIterator<Item = TypeId>) {
            let lock = &mut self.state.write().expect("Not poisoned").messages;
            for typeid in typeids {
                let actor_ids_able_to_process_msg =
                    lock.get_mut(&typeid).expect("published when add_handler");
                let removed = actor_ids_able_to_process_msg.remove(&self.id);
                assert!(removed, "Expected to remove itself from list");
            }
        }
    }
    impl Drop for DeviceReleaser {
        fn drop(&mut self) {
            let mut lock = self.state.write().expect("Not poisoned");
            lock.devices.remove(&self.id);
        }
    }
}

#[cfg(feature = "tokio")]
type Task = tokio::task::JoinHandle<()>;

#[cfg(not(feature = "tokio"))]
type Task = futures::future::BoxFuture<'static, ()>;

type HandlerClosureResponse = Option<Task>;

pub trait ActorExecutionStrategy<TState> {
    fn execute<'a>(
        &'a self,
        handler: &'a dyn MessageHandler<TState>,
        state: TState,
        untyped_message: BoxMessage,
    ) -> BoxFuture<'a, (TState, HandlerClosureResponse)>;
}

struct AlwaysHandleStrategy;
impl<TState> ActorExecutionStrategy<TState> for AlwaysHandleStrategy {
    fn execute<'a>(
        &'a self,
        handler: &'a dyn MessageHandler<TState>,
        state: TState,
        untyped_message: BoxMessage,
    ) -> BoxFuture<'a, (TState, HandlerClosureResponse)> {
        handler.handle(state, untyped_message)
    }
}

#[allow(clippy::type_complexity)]
pub struct ActorDevice<TState> {
    receiver: mpsc::Receiver<(TypeId, BoxMessage)>, // Contains MessageWithResponse<TMsg>
    post: ActorDevicePostExecute<TState>,
    pending_tasks: FuturesUnordered<Task>,
}

pub struct ActorDevicePostExecute<TState> {
    manager: releaser::DeviceReleaser,
    handlers: HashMap<TypeId, Box<dyn MessageHandler<TState>>>,
}

impl<TState> ActorDevice<TState> {
    fn new(
        receiver: mpsc::Receiver<(TypeId, BoxMessage)>,
        manager: releaser::DeviceReleaser,
    ) -> Self {
        ActorDevice {
            receiver,
            post: ActorDevicePostExecute {
                handlers: Default::default(),
                manager,
            },
            pending_tasks: Default::default(),
        }
    }
}

impl<TState: 'static + Send + Sync> ActorDevice<TState> {
    pub fn add_handler<TMsg: ActorMessage>(
        mut self,
        closure: impl for<'a> AsyncHandlerClosure<'a, TState, TMsg> + 'static + Send + Sync + Clone,
    ) -> Self {
        let typeid = TypeId::of::<TMsg>();
        self.post.handlers.insert(
            typeid,
            Box::new(AsyncMessageHandler {
                closure,
                phantom: PhantomData,
            }),
        );
        self.post.manager.publish_message(typeid);
        self
    }

    #[cfg(any(feature = "tokio", feature = "rayon", test))]
    pub fn add_sync_handler<TMsg: ActorMessage>(
        mut self,
        h: fn(&mut TState, TMsg) -> ActorResult<TMsg>,
    ) -> Self {
        let typeid = TypeId::of::<TMsg>();
        self.post.handlers.insert(
            typeid,
            Box::new(SyncMessageHandler::<TState, TMsg>(h, PhantomData)),
        );
        self.post.manager.publish_message(typeid);
        self
    }

    #[cfg(feature = "unstable")]
    pub fn add_waiting_handler_till_abort<TMsg: ActorMessage>(self) -> Self {
        async fn blocking<TState, TMsg: ActorMessage>(
            _state: &mut TState,
            _msg: TMsg,
            r: AbortRegistration,
        ) -> ActorResult<TMsg> {
            Abortable::new(poll_fn::<(), _>(|_| Poll::Pending), r)
                .await
                .ok();
            Err(ActorError::Aborted)
        }
        self.add_handler(WithAbort::new(blocking::<TState, TMsg>))
    }

    #[deprecated = "Blocking got a new notion with blocking_handlers... The name became confusing"]
    #[cfg(feature = "unstable")]
    pub fn add_blocking_handler_till_abort<TMsg: ActorMessage>(self) -> Self {
        self.add_waiting_handler_till_abort::<TMsg>()
    }

    pub async fn execute(self, state: TState) -> TState {
        self.execute_with_strategy(state, AlwaysHandleStrategy)
            .await
    }

    pub async fn execute_with_strategy(
        mut self,
        mut state: TState,
        strategy: impl ActorExecutionStrategy<TState>,
    ) -> TState {
        while let Some((typeid, untyped_message)) = self.receiver.next().await {
            if let Some(available_handler) = self.post.handlers.get(&typeid) {
                let fut = strategy.execute(available_handler.as_ref(), state, untyped_message);
                pin_mut!(fut);

                let mut infinite_pending =
                    (&mut self.pending_tasks).chain(futures::stream::pending());

                state = loop {
                    if let Either::Left(((state, maybe_task), _)) =
                        futures::future::select(&mut fut, infinite_pending.next()).await
                    {
                        if let Some(task) = maybe_task {
                            self.pending_tasks.push(task);
                        }
                        break state;
                    }
                }
            }
        }

        while self.pending_tasks.next().await.is_some() {}
        state
    }
}

impl<T> Drop for ActorDevicePostExecute<T> {
    fn drop(&mut self) {
        self.manager
            .revoke_message_responsibility(self.handlers.keys().copied());
    }
}

#[cfg(test)]
mod tests {
    use std::{task::Poll, time::Duration};

    use futures::{
        future::{join_all, poll_fn},
        stream::{AbortRegistration, Abortable},
        FutureExt,
    };
    use tokio::time::sleep;

    use super::*;

    struct I32Message(i32);

    impl ActorMessage for I32Message {
        type Output = i64;
        type Error = String;
    }

    #[tokio::test]
    async fn cancellable_task_gets_cancelled() {
        let system = Arc::new(ActorSystem::new());
        let abort_system = system.clone();
        async fn handler(
            state: &mut i32,
            _msg: I32Message,
            reg: AbortRegistration,
        ) -> Result<i64, ActorError<String>> {
            assert!(Abortable::new(poll_fn(|_| Poll::<()>::Pending), reg)
                .await
                .is_err());
            Ok(*state as i64)
        }
        let id = DeviceId::new_v4();
        let system = system
            .register(id)
            .add_handler(WithAbort::new(handler))
            .execute(42)
            .map(|_| ());

        assert_eq!(
            tokio::time::timeout(
                Duration::from_secs(10),
                futures::future::join(system, async {
                    sleep(Duration::from_millis(1)).await;
                    {
                        let _aborted = abort_system.ask(id, I32Message(42));
                    }
                    abort_system.forget_senders();
                }),
            )
            .await,
            Ok(((), ()))
        );
    }

    #[tokio::test]
    async fn shutdown_gracefully() {
        let system = Arc::new(ActorSystem::new());
        let abort_system = system.clone();

        let runners = std::iter::once(
            async {
                sleep(Duration::from_millis(1)).await;
                abort_system.forget_senders();
            }
            .boxed(),
        )
        .chain((0..100).map(move |_| {
            let system = system.clone();
            async move {
                let id = DeviceId::new_v4();
                async fn handler(
                    state: &mut i32,
                    _msg: I32Message,
                ) -> Result<i64, ActorError<String>> {
                    Ok(*state as i64)
                }

                system.register(id).add_handler(handler).execute(1).await;
            }
            .boxed()
        }))
        .collect::<Vec<_>>();

        tokio::time::timeout(Duration::from_secs(10), join_all(runners))
            .await
            .expect("ActorSystem should stop gracefully");
    }

    #[tokio::test]
    async fn remove_message_lookup_device() {
        let system = ActorSystem::new();
        let id = DeviceId::new_v4();
        async fn handler(state: &mut i32, _msg: I32Message) -> Result<i64, ActorError<String>> {
            Ok(*state as i64)
        }
        let runner = system.register(id).add_handler(handler).execute(1);
        assert_eq!(
            system
                .list_devices_for_message_type::<I32Message>()
                .into_iter()
                .collect::<Vec<_>>(),
            vec![id]
        );
        drop(runner);
        assert_eq!(
            None,
            system
                .list_devices_for_message_type::<I32Message>()
                .into_iter()
                .next()
        );
    }

    #[tokio::test]
    async fn remove_device_after_drop() {
        let system = ActorSystem::new();
        let device_id = DeviceId::new_v4();
        async fn handler(state: &mut i32, _msg: I32Message) -> Result<i64, ActorError<String>> {
            Ok(*state as i64)
        }
        drop(system.register(device_id).add_handler(handler).execute(1));

        assert_eq!(
            system.get_untyped_sender(device_id).unwrap_err(),
            ActorErrorUnknownDevice::UnknownDeviceId {
                device_id,
                details: "No message queue for this device".into()
            }
        );
    }

    #[tokio::test]
    async fn handle_messages() {
        let system = ActorSystem::new();
        let id = DeviceId::new_v4();

        async fn handler(state: &mut State, msg: I32Message) -> Result<i64, ActorError<String>> {
            state.0 = msg.0;
            Ok(msg.0 as i64)
        }

        struct State(i32);

        let (state, _) = futures::future::join(
            system.register(id).add_handler(handler).execute(State(0)),
            async move {
                tokio::time::sleep(Duration::from_micros(10)).await;
                let result = system.ask(id, I32Message(42)).await;
                assert_eq!(42i64, result.unwrap());
                system.forget_senders();
            },
        )
        .await;
        assert_eq!(state.0, 42);
    }

    #[tokio::test]
    async fn handle_sync_messages() {
        let system = ActorSystem::new();
        let id = DeviceId::new_v4();

        fn handler(state: &mut State, msg: I32Message) -> Result<i64, ActorError<String>> {
            state.0 = msg.0;
            Ok(msg.0 as i64)
        }

        struct State(i32);

        let (state, _) = futures::future::join(
            system
                .register(id)
                .add_sync_handler(handler)
                .execute(State(0)),
            async move {
                tokio::time::sleep(Duration::from_micros(10)).await;
                let result = system.ask(id, I32Message(42)).await;
                assert_eq!(42i64, result.unwrap());
                system.forget_senders();
            },
        )
        .await;
        assert_eq!(state.0, 42);
    }

    #[tokio::test]
    async fn handle_message_with_error() {
        let system = ActorSystem::new();
        let id = DeviceId::new_v4();

        const ERROR_MSG: &str = "Something went wrong";

        async fn handler(_state: &mut State, _msg: I32Message) -> Result<i64, ActorError<String>> {
            Err(ActorError::Custom(ERROR_MSG.to_string()))
        }

        struct State(i32);

        futures::future::join(
            system.register(id).add_handler(handler).execute(State(0)),
            async move {
                tokio::time::sleep(Duration::from_micros(10)).await;
                let result = system.ask(id, I32Message(42)).await;
                assert_eq!(
                    ActorError::Custom(ERROR_MSG.to_string()),
                    result.unwrap_err()
                );
                system.forget_senders();
            },
        )
        .await;
    }

    #[tokio::test]
    async fn handle_unknown_device() {
        let system = ActorSystem::new();
        let device_id = DeviceId::new_v4();
        assert_eq!(
            system.ask(device_id, I32Message(42)).await,
            Err(ActorError::UnknownDevice(
                ActorErrorUnknownDevice::UnknownDeviceId {
                    device_id,
                    details: "No message queue for this device".into()
                }
            ))
        );
    }

    #[tokio::test]
    async fn get_devices_for_messages() {
        async fn handler<TMsg>(_state: &mut i32, _msg: TMsg) -> Result<(), ActorError<()>> {
            Ok(())
        }

        struct MsgA {}
        impl ActorMessage for MsgA {
            type Output = ();
            type Error = ();
        }
        struct MsgB {}
        impl ActorMessage for MsgB {
            type Output = ();
            type Error = ();
        }
        let actor_a_id = DeviceId::new_v4();
        let actor_b_id = DeviceId::new_v4();
        let actor_ab_id = DeviceId::new_v4();
        let system = ActorSystem::new();
        let _actor_a = system
            .register(actor_a_id)
            .add_handler(handler::<MsgA>)
            .execute(1);

        let _actor_b = system
            .register(actor_b_id)
            .add_handler(handler::<MsgB>)
            .execute(1);

        {
            let _actor_ab = system
                .register(actor_ab_id)
                .add_handler(handler::<MsgA>)
                .add_handler(handler::<MsgB>)
                .execute(1);
            assert_eq!(
                [actor_a_id, actor_ab_id]
                    .into_iter()
                    .collect::<HashSet<_>>(),
                system.list_devices_for_message_types([std::any::TypeId::of::<MsgA>()])
            );
            assert_eq!(
                [actor_b_id, actor_ab_id]
                    .into_iter()
                    .collect::<HashSet<_>>(),
                system.list_devices_for_message_types([std::any::TypeId::of::<MsgB>()])
            );
            assert_eq!(
                [actor_ab_id].into_iter().collect::<HashSet<_>>(),
                system.list_devices_for_message_types([
                    std::any::TypeId::of::<MsgA>(),
                    std::any::TypeId::of::<MsgB>()
                ])
            );
        }
        assert_eq!(
            [actor_b_id].into_iter().collect::<HashSet<_>>(),
            system.list_devices_for_message_types([std::any::TypeId::of::<MsgB>()]),
            "Should only contain actor_b_id, as actor_ab_id should be removed when dropping the runner"
        );
    }

    #[tokio::test]
    async fn handle_unknown_message() {
        let system = ActorSystem::new();
        let id = DeviceId::new_v4();
        tokio::select! {
        _ = system.register(id).execute(42) => { panic!("Should not terminate"); },
        _ = async {
            let result = system.ask(id, I32Message(42)).await;
            if let Err(ActorError::UnknownMessageType(x)) = result {
                assert!(x.contains("I32Message"), "{:?}", x);
            } else {
                panic!("{:?}", result);
            }
        } => {}};
    }
}
