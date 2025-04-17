use std::future::Future;

use super::{ActorMessage, ActorResult, HandlerClosureContext, HandlerClosureResponse};

pub trait HandlerResult<TMsg: ActorMessage>: 'static + Send {
    fn handle_as_result(
        self,
        response_channel: HandlerClosureContext<TMsg>,
    ) -> HandlerClosureResponse;
}

impl<TMsg: ActorMessage> HandlerResult<TMsg> for ActorResult<TMsg> {
    fn handle_as_result(self, ctx: HandlerClosureContext<TMsg>) -> HandlerClosureResponse {
        ctx.respond(self);
        None
    }
}

/// Used to calculate the response without access to &mut state.
/// This allows time-consuming tasks to answer requests without blocking other messages for the same Actor
pub struct Step2<T>(pub T);

impl<TFut: Future<Output = ActorResult<TMsg>> + 'static + Send, TMsg: ActorMessage>
    HandlerResult<TMsg> for Step2<TFut>
{
    fn handle_as_result(self, ctx: HandlerClosureContext<TMsg>) -> HandlerClosureResponse {
        let fut = async {
            ctx.respond(self.0.await);
        };

        #[cfg(feature = "tokio")]
        let res = tokio::task::spawn(fut);
        #[cfg(not(feature = "tokio"))]
        let res = futures_util::future::FutureExt::boxed(fut);

        Some(res)
    }
}
