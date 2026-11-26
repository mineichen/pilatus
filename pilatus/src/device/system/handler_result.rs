use std::future::Future;

use futures_util::future::{select, Either};

use super::{responder::ActorRequestResponder, ActorMessage, ActorResult, HandlerClosureResponse};

pub trait HandlerResult<TMsg: ActorMessage>: 'static + Send {
    fn handle_as_result(self, responder: ActorRequestResponder<TMsg>) -> HandlerClosureResponse;
}

impl<TMsg: ActorMessage> HandlerResult<TMsg> for ActorResult<TMsg> {
    fn handle_as_result(self, ctx: ActorRequestResponder<TMsg>) -> HandlerClosureResponse {
        ctx.respond(self);
        None
    }
}

/// Used to calculate the response without access to &mut state.
/// This allows time-consuming tasks to answer requests without blocking other messages for the same Actor.
///
/// Returned by a `WithAbort` handler, its `AbortRegistration` is triggered when the request is
/// cancelled while the future runs. The future still runs to its end, so it can clean up.
pub struct Step2<T>(pub T);

impl<TFut: Future<Output = ActorResult<TMsg>> + 'static + Send, TMsg: ActorMessage>
    HandlerResult<TMsg> for Step2<TFut>
{
    fn handle_as_result(self, mut ctx: ActorRequestResponder<TMsg>) -> HandlerClosureResponse {
        let fut = async move {
            let result = match ctx.take_abort() {
                Some(abort) => {
                    let mut fut = std::pin::pin!(self.0);
                    match select(fut.as_mut(), ctx.cancellation()).await {
                        Either::Left((result, _)) => result,
                        Either::Right(((), _)) => {
                            abort.abort();
                            fut.await
                        }
                    }
                }
                None => self.0.await,
            };
            ctx.respond(result);
        };

        #[cfg(feature = "tokio")]
        let res = tokio::task::spawn(fut);
        #[cfg(not(feature = "tokio"))]
        let res = futures_util::future::FutureExt::boxed(fut);

        Some(res)
    }
}
