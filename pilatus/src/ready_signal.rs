//! Lets a running task (e.g. a `HostedService`) tell others, that it is ready.

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use futures_channel::oneshot::{self, Canceled};

/// Is triggered by a task once it is ready (e.g. a webserver, which accepts connections).
///
/// The task keeps running after signaling. [`ReadySignal::signal`] consumes the signal and resolves the [`ReadyListener`].
/// If it is dropped without signaling (e.g. the task failed), the listener resolves with [`Canceled`].
#[derive(Debug)]
pub struct ReadySignal(oneshot::Sender<()>);

impl ReadySignal {
    /// Creates a signal and the listener, which resolves once the signal is triggered.
    ///
    /// Mostly used by runtimes. In tests, it allows calling a hosted service function directly:
    /// ```
    /// # futures_util::FutureExt::now_or_never(async {
    /// let (ready, listener) = pilatus::ReadySignal::new();
    /// ready.signal();
    /// assert_eq!(Ok(()), listener.await);
    /// # }).unwrap();
    /// ```
    pub fn new() -> (Self, ReadyListener) {
        let (tx, rx) = oneshot::channel();
        (Self(tx), ReadyListener(rx))
    }

    /// Signals that the task is ready.
    pub fn signal(self) {
        let _ignore_absent_listener = self.0.send(());
    }
}

/// Resolves once the corresponding [`ReadySignal`] is triggered.
#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct ReadyListener(oneshot::Receiver<()>);

impl Future for ReadyListener {
    type Output = Result<(), Canceled>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}

#[cfg(test)]
mod tests {
    use futures_util::FutureExt;

    use super::*;

    #[test]
    fn signal_resolves_listener() {
        let (ready, mut listener) = ReadySignal::new();
        assert_eq!(None, (&mut listener).now_or_never());
        ready.signal();
        assert_eq!(Some(Ok(())), listener.now_or_never());
    }

    #[test]
    fn dropping_without_signal_resolves_with_error() {
        let (ready, listener) = ReadySignal::new();
        drop(ready);
        assert_eq!(Some(Err(Canceled)), listener.now_or_never());
    }

    #[test]
    fn signal_without_listener_is_ignored() {
        let (ready, listener) = ReadySignal::new();
        drop(listener);
        ready.signal();
    }
}
