use futures::{
    channel::oneshot,
    future::BoxFuture,
    stream::{AbortHandle, AbortRegistration},
    Future, FutureExt,
};

use super::{ActorMessage, ActorResult, HandlerClosureResponse, HandlerResult};

pub trait HandlerClosure<'a, TState, TMsg: ActorMessage> {
    type Fut: Future<Output = Self::Result> + 'a + Send;
    type Result: HandlerResult<TMsg>;
    type FinalFut: Future<Output = HandlerClosureResponse> + 'a + Send;

    fn call(
        &self,
        state: &'a mut TState,
        msg: TMsg,
        c: HandlerClosureContext<TMsg>,
    ) -> Self::FinalFut;
}

pub struct HandlerClosureContext<TMsg: ActorMessage> {
    pub(super) response_channel: oneshot::Sender<ActorResult<TMsg>>,
}

impl<'a, TState, TMsg, THandlerResult, TFut, TFn> HandlerClosure<'a, TState, TMsg> for TFn
where
    TState: 'a,
    TMsg: ActorMessage,
    THandlerResult: HandlerResult<TMsg>,
    TFut: Future<Output = THandlerResult> + 'a + Send,
    TFn: Fn(&'a mut TState, TMsg) -> TFut,
{
    type Fut = TFut;
    type Result = THandlerResult;
    type FinalFut = BoxFuture<'a, HandlerClosureResponse>;

    fn call(
        &self,
        state: &'a mut TState,
        msg: TMsg,
        response_channel: HandlerClosureContext<TMsg>,
    ) -> BoxFuture<'a, HandlerClosureResponse> {
        let result = (self)(state, msg);
        async { result.await.handle_as_result(response_channel) }.boxed()
    }
}

#[derive(Clone)]
pub struct WithAbort<TFn>(TFn);
impl<TFn> WithAbort<TFn> {
    pub fn new(t: TFn) -> Self {
        Self(t)
    }
}

impl<'a, TState, TMsg, THandlerResult, TFut, TFn> HandlerClosure<'a, TState, TMsg>
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

    fn call(
        &self,
        state: &'a mut TState,
        msg: TMsg,
        mut ctx: HandlerClosureContext<TMsg>,
    ) -> BoxFuture<'a, HandlerClosureResponse> {
        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        let future = self.0(state, msg, abort_registration).fuse();
        async move {
            futures::pin_mut!(future);
            futures::select! {
                result = future => {
                    result.handle_as_result(ctx)
                },
                _ = ctx.response_channel.cancellation().fuse() => {
                    abort_handle.abort();
                    future.await.handle_as_result(ctx)
                }
            }
        }
        .boxed()
    }
}
