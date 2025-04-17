use std::future::Ready;

use futures_util::{
    future::{join, select, BoxFuture, Either, Join, Map},
    stream::{AbortHandle, AbortRegistration},
    Future, FutureExt,
};

use super::{ActorMessage, HandlerClosureResponse, HandlerResult, Task};

pub trait AsyncHandlerClosure<'a, TState, TMsg: ActorMessage> {
    type Fut: Future<Output = Self::Result> + 'a + Send;
    type Result: HandlerResult<TMsg>;
    type FinalFut: Future<Output = HandlerClosureResponse> + 'a + Send;

    /// Use return impl Future<> as soon as this issue is resolved:
    /// this is a known limitation that will be removed in the future (see issue #100013 <https://github.com/rust-lang/rust/issues/100013> for more information)rustcClick
    fn call(
        &self,
        state: &'a mut TState,
        msg: TMsg,
        c: HandlerClosureContext<TMsg>,
    ) -> Self::FinalFut;
}

pub use internal::HandlerClosureContext;
mod internal {
    use tracing::trace;

    use crate::device::{ActorMessage, ActorResult};

    pub struct HandlerClosureContext<TMsg: ActorMessage> {
        start_time: std::time::Instant,
        response_channel: futures_channel::oneshot::Sender<ActorResult<TMsg>>,
    }

    impl<TMsg: ActorMessage> HandlerClosureContext<TMsg> {
        pub fn new(response_channel: futures_channel::oneshot::Sender<ActorResult<TMsg>>) -> Self {
            Self {
                response_channel,
                start_time: std::time::Instant::now(),
            }
        }

        pub fn respond(self, r: ActorResult<TMsg>) {
            let r = self.response_channel.send(r);
            trace!(
                "Responding to {} after {:?}{}",
                std::any::type_name::<TMsg>(),
                self.start_time.elapsed(),
                if r.is_err() {
                    "(but listener was gone)"
                } else {
                    ""
                }
            );
        }

        pub fn cancellation(
            &mut self,
        ) -> futures_channel::oneshot::Cancellation<'_, ActorResult<TMsg>> {
            self.response_channel.cancellation()
        }
    }
}

impl<'a, TState, TMsg, THandlerResult, TFut, TFn> AsyncHandlerClosure<'a, TState, TMsg> for TFn
where
    TState: 'a,
    TMsg: ActorMessage,
    THandlerResult: HandlerResult<TMsg>,
    TFut: Future<Output = THandlerResult> + 'a + Send,
    TFn: Fn(&'a mut TState, TMsg) -> TFut,
{
    type Fut = TFut;
    type Result = THandlerResult;
    type FinalFut = Map<
        Join<TFut, Ready<HandlerClosureContext<TMsg>>>,
        fn((THandlerResult, HandlerClosureContext<TMsg>)) -> Option<Task>,
    >;

    fn call(
        &self,
        state: &'a mut TState,
        msg: TMsg,
        response_channel: HandlerClosureContext<TMsg>,
    ) -> Self::FinalFut {
        let result = (self)(state, msg);
        join(result, std::future::ready(response_channel))
            .map(|(x, response_channel)| x.handle_as_result(response_channel))
    }
}

#[derive(Clone)]
pub struct WithAbort<TFn>(TFn);
impl<TFn> WithAbort<TFn> {
    pub fn new(t: TFn) -> Self {
        Self(t)
    }
}

impl<'a, TState, TMsg, THandlerResult, TFut, TFn> AsyncHandlerClosure<'a, TState, TMsg>
    for WithAbort<TFn>
where
    TState: 'static,
    TMsg: ActorMessage,
    THandlerResult: HandlerResult<TMsg>,
    TFut: Future<Output = THandlerResult> + 'a + Send,
    TFn: Fn(&'a mut TState, TMsg, AbortRegistration) -> TFut,
{
    type Fut = TFut;
    type Result = THandlerResult;
    type FinalFut = BoxFuture<'a, HandlerClosureResponse>;

    // Remove Boxing once (see issue #100013 <https://github.com/rust-lang/rust/issues/100013>) is resolved
    // This is difficult (impossible?) to write without a custom Future, as ctx captured by the by the second future.
    // If future returns before, it cant access the context
    fn call(
        &self,
        state: &'a mut TState,
        msg: TMsg,
        mut ctx: HandlerClosureContext<TMsg>,
    ) -> BoxFuture<'a, HandlerClosureResponse> {
        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        let future = self.0(state, msg, abort_registration).fuse();

        async move {
            select(std::pin::pin!(future), ctx.cancellation())
                .then(move |r| match r {
                    Either::Left((x, _)) => std::future::ready(x).left_future(),
                    Either::Right((_, other)) => {
                        abort_handle.abort();
                        other.right_future()
                    }
                })
                .await
                .handle_as_result(ctx)
        }
        .boxed()
    }
}
