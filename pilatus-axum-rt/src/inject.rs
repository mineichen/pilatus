use std::{
    fmt::Debug,
    task::{self, Poll},
};

use anyhow::Result;
use minfac::WeakServiceProvider;
use pilatus_axum::http::Request;

use tower::Service;

impl<S, RequestBody> Service<Request<RequestBody>> for InjectService<S>
where
    S: Service<Request<RequestBody>>,
    RequestBody: Debug,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&mut self, mut request: Request<RequestBody>) -> Self::Future {
        request.extensions_mut().insert(self.provider.clone());
        self.service.call(request)
    }
}

#[derive(Clone)]
pub(crate) struct InjectLayer(pub WeakServiceProvider);

impl<S> tower::Layer<S> for InjectLayer {
    type Service = InjectService<S>;

    fn layer(&self, service: S) -> Self::Service {
        InjectService {
            provider: self.0.clone(),
            service,
        }
    }
}

#[derive(Clone)]
pub struct InjectService<S> {
    provider: WeakServiceProvider,
    service: S,
}
