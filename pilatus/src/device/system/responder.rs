use futures_util::stream::AbortHandle;
use tracing::trace;

use crate::device::{ActorMessage, ActorResult};

pub struct ActorRequestResponder<TMsg: ActorMessage> {
    start_time: std::time::Instant,
    response_channel: futures_channel::oneshot::Sender<ActorResult<TMsg>>,
    /// Aborts what a `WithAbort` handler continues to calculate (e.g. in `Step2`), when the
    /// request is cancelled
    abort: Option<AbortHandle>,
}

impl<TMsg: ActorMessage> ActorRequestResponder<TMsg> {
    pub fn new(response_channel: futures_channel::oneshot::Sender<ActorResult<TMsg>>) -> Self {
        Self {
            response_channel,
            start_time: std::time::Instant::now(),
            abort: None,
        }
    }

    pub(super) fn with_abort(self, abort: AbortHandle) -> Self {
        Self {
            abort: Some(abort),
            ..self
        }
    }

    pub(super) fn take_abort(&mut self) -> Option<AbortHandle> {
        self.abort.take()
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
