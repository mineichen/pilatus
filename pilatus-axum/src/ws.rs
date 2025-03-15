/// Pilatus provides it's own WebSocketUpgrade to avoid running Websockets when changeing recipes
use std::{future::Future, sync::Arc};

use axum::{
    extract::{ws, FromRequestParts},
    http::{self, request::Parts, StatusCode},
};
use futures::{
    stream::{AbortHandle, AbortRegistration},
    FutureExt,
};

use super::extract::InjectRegistered;

pub struct WebSocketUpgrade {
    store: Arc<dyn WebSocketDropperService>,
    inner: ws::WebSocketUpgrade,
}

impl WebSocketUpgrade {
    // Get access to the raw WebSocketUpgrade, which is not expected to end
    // when switching ActiveRecipe
    pub fn into_inner(self) -> ws::WebSocketUpgrade {
        self.inner
    }

    pub fn on_upgrade<C, Fut>(self, callback: C) -> axum::http::Response<axum::body::Body>
    where
        C: FnOnce(ws::WebSocket) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let dropper = self.store.create_dropper();
        self.inner.on_upgrade(move |s| {
            let dropper = dropper;
            callback(s).map(move |x| {
                drop(dropper);
                x
            })
        })
    }
}

// Receive handles which has to be Dropp
pub trait WebSocketDropperService: Send + Sync {
    fn create_dropper(&self) -> Dropper;
}

impl<S: Send + Sync> FromRequestParts<S> for WebSocketUpgrade {
    type Rejection = (http::StatusCode, String);

    async fn from_request_parts(req: &mut Parts, s: &S) -> Result<Self, Self::Rejection> {
        let InjectRegistered(store) =
            InjectRegistered::<Arc<dyn WebSocketDropperService>>::from_request_parts(req, s)
                .await
                .map_err(|(code, msg)| (code, msg.to_owned()))?;

        let inner = ws::WebSocketUpgrade::from_request_parts(req, s)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        Ok(WebSocketUpgrade { inner, store })
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct Dropper(Arc<InnerDropper>);

impl Dropper {
    pub fn pair() -> (Self, AbortRegistration) {
        let (handle, reg) = futures::future::AbortHandle::new_pair();
        (Self(Arc::new(InnerDropper(handle))), reg)
    }
}

struct InnerDropper(AbortHandle);

impl Drop for InnerDropper {
    fn drop(&mut self) {
        self.0.abort();
    }
}
