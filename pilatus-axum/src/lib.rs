mod abort;
mod dependency_provider;
mod device_response;
#[cfg(feature = "engineering")]
pub mod image;
mod inject;
mod minfac_extensions;
mod routing;
mod ws;

use futures::{channel::oneshot, future::Shared};
use std::net::SocketAddr;

pub use abort::AbortServiceInterface;
pub use axum::{
    body::{Bytes, StreamBody},
    http,
    response::{sse, AppendHeaders, Html, IntoResponse},
};
pub use dependency_provider::DependencyProvider;
pub use device_response::{DeviceJsonResponse, DeviceMessageJsonResponse, DeviceResponse};
pub use minfac_extensions::ServiceCollectionExtensions;
pub use routing::{MethodRouter, Router};

pub mod extract {
    pub struct Inject<T: minfac::Resolvable>(pub T::ItemPreChecked);
    pub struct InjectRegistered<T: std::any::Any>(pub T);
    pub use super::abort::Abort;
    pub struct InjectAll<T: std::any::Any>(pub ServiceIterator<Registered<T>>);
    pub use axum::extract::{BodyStream, FromRequestParts, Json, Path, Query};
    use minfac::{Registered, ServiceIterator};

    pub mod ws {
        pub use super::super::ws::{Dropper, WebSocketDropperService, WebSocketUpgrade};
        pub use axum::extract::ws::{Message, WebSocket};
    }
}

pub type MinfacRouter = private::MoveOutOnClone<axum::Router>;

pub struct Stats {
    socket: Shared<oneshot::Receiver<SocketAddr>>,
}
impl Stats {
    pub fn new(socket: Shared<oneshot::Receiver<SocketAddr>>) -> Self {
        Self { socket }
    }
    pub async fn socket_addr(&self) -> SocketAddr {
        self.socket
            .clone()
            .await
            .expect("always resolved when server started")
    }
}

pub struct DeviceTopicDiscovery {
    pub device_type: &'static str,
    pub topic: &'static str,
    pub path: &'static str,
}

impl DeviceTopicDiscovery {
    pub fn new(device_type: &'static str, topic: &'static str, path: &'static str) -> Self {
        Self {
            device_type,
            topic,
            path,
        }
    }
}

mod private {
    use std::sync::Mutex;

    // Todo: Unify with OnceExtractor
    pub struct MoveOutOnClone<T>(Mutex<Option<T>>);
    impl<T> MoveOutOnClone<T> {
        pub(super) fn new(inner: T) -> Self {
            Self(Mutex::new(Some(inner)))
        }
        pub fn unchecked_extract(&self) -> T {
            let value = { self.0.lock().expect("Lock is never poisoned").take() };
            value.expect("Value was extracted multiple times")
        }
    }
    impl<T> Clone for MoveOutOnClone<T> {
        fn clone(&self) -> Self {
            let mut lock = self.0.lock().expect("Lock is never poisoned");
            Self(Mutex::new(lock.take()))
        }
    }
}
