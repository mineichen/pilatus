use std::future::Ready;

use futures_util::{
    future::{join, select, BoxFuture, Either, Join, Map},
    stream::{AbortHandle, AbortRegistration},
    Future, FutureExt,
};

use super::{
    responder::ActorRequestResponder, ActorMessage, HandlerClosureResponse, HandlerResult, Task,
};

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
        c: ActorRequestResponder<TMsg>,
    ) -> Self::FinalFut;
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
        Join<TFut, Ready<ActorRequestResponder<TMsg>>>,
        fn((THandlerResult, ActorRequestResponder<TMsg>)) -> Option<Task>,
    >;

    fn call(
        &self,
        state: &'a mut TState,
        msg: TMsg,
        response_channel: ActorRequestResponder<TMsg>,
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
        mut ctx: ActorRequestResponder<TMsg>,
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
