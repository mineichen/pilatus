/// Usually, the system shuts down when receiving a ctrl-c signal
///
/// For testing, the function `register_test_services` can be used.
/// When register_test_services is used, SystemShutdown terminates too if SystemTerminator.shutdown() is called
use std::{
    pin::Pin,
    task::{self, Poll},
};

use futures::{future::Shared, stream::AbortHandle, Future, FutureExt};

type InnerPrivateState =
    Shared<Pin<std::boxed::Box<(dyn futures::Future<Output = ()> + 'static + Send + Sync)>>>;

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
