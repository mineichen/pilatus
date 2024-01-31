use futures::{stream::Abortable, Future, FutureExt};

/// In contrast to futures::stream::AbortableRegistration, this could be used to cancel multiple tasks
pub struct AbortRegistration {
    unpin_abort: futures::stream::Abortable<std::future::Pending<()>>,
}

impl AbortRegistration {
    pub fn new_pair() -> (futures::stream::AbortHandle, Self) {
        let (handle, reg) = futures::stream::AbortHandle::new_pair();
        (
            handle,
            Self {
                unpin_abort: Abortable::new(std::future::pending(), reg),
            },
        )
    }
    pub async fn abortable<TFut: Future>(
        &mut self,
        fut: TFut,
    ) -> Result<TFut::Output, futures::stream::Aborted> {
        let pinned = std::pin::pin!(fut);
        match futures::future::select(&mut self.unpin_abort, pinned).await {
            futures::future::Either::Left(_) => Err(futures::future::Aborted),
            futures::future::Either::Right((x, _)) => Ok(x),
        }
    }
}

impl Future for AbortRegistration {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.unpin_abort.poll_unpin(cx).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use futures::stream::Aborted;

    use super::*;

    #[tokio::test]
    async fn abort_simple() {
        let (handle, mut reg) = AbortRegistration::new_pair();
        assert_eq!(42, reg.abortable(async { 42 }).await.unwrap());
        handle.abort();
        assert_eq!(
            Err(Aborted),
            reg.abortable(std::future::pending::<()>()).await
        );
    }
}
