use std::{
    pin::Pin,
    task::{self, Poll},
};

use futures::Future;
use pin_project::pin_project;

#[pin_project]
pub struct MetadataFuture<TMeta, T>(Option<TMeta>, #[pin] T);
impl<TMeta, T> MetadataFuture<TMeta, T> {
    pub fn new(meta: TMeta, fut: T) -> Self {
        Self(Some(meta), fut)
    }

    pub fn get_meta(&self) -> &TMeta {
        self.0
            .as_ref()
            .expect("get_meta cannot be called when the future finished")
    }
}

impl<TMeta, T: Future> Future for MetadataFuture<TMeta, T> {
    type Output = (TMeta, T::Output);

    fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        this.1.poll(cx).map(|x| {
            (
                this.0
                    .take()
                    .expect("Mustn't be polled after returning Ready"),
                x,
            )
        })
    }
}
