/// Usually, the system shuts down when receiving a ctrl-c signal
///
/// For testing, the function `register_test_services` can be used.
/// When register_test_services is used, SystemShutdown terminates too if SystemTerminator.shutdown() is called
use std::{
    future::Future,
    pin::Pin,
    task::{self, Poll},
};

use futures_util::{
    future::{select, Either, Shared},
    stream::AbortHandle,
    FutureExt,
};

type InnerPrivateState =
    Shared<Pin<std::boxed::Box<dyn Future<Output = ()> + 'static + Send + Sync>>>;

// Should be used rather than ctrl_c to allow stopping during tests
#[derive(Clone)]
pub struct SystemShutdown(InnerPrivateState);

impl SystemShutdown {
    pub fn new(inner: InnerPrivateState) -> Self {
        Self(inner)
    }
}

impl Future for SystemShutdown {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
        self.0.poll_unpin(cx)
    }
}

pub struct SystemTerminator(AbortHandle);

impl SystemTerminator {
    pub fn new(handle: AbortHandle) -> Self {
        Self(handle)
    }
    pub fn shutdown(&self) {
        self.0.abort();
    }
}

/// Returned by [`SystemShutdown::run_or_shutdown`] if the system shut down before the future completed
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("System is shutting down")]
pub struct ShutdownError;

impl SystemShutdown {
    /// Drives `other` to completion, unless the system shuts down first.
    ///
    /// If both are ready at the same time, the result of `other` is preferred.
    pub async fn run_or_shutdown<F: Future>(
        &mut self,
        other: F,
    ) -> Result<F::Output, ShutdownError> {
        match select(std::pin::pin!(other), self).await {
            Either::Left((output, _)) => Ok(output),
            Either::Right(_) => Err(ShutdownError),
        }
    }
}
