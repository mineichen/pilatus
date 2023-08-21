mod abort;
mod dependency_provider;
#[cfg(feature = "engineering")]
pub mod image;
mod inject;
mod into_response;
mod minfac_extensions;
mod routing;
mod web_component;
mod ws;

use std::net::SocketAddr;

use futures::{channel::oneshot, future::Shared};

pub use abort::AbortServiceInterface;
pub use axum::{
    body::{Bytes, StreamBody},
    http,
    response::{sse, AppendHeaders, Html, IntoResponse, Response},
};
pub use dependency_provider::DependencyProvider;
pub use into_response::*;
pub use minfac_extensions::ServiceCollectionExtensions;
pub use routing::{MethodRouter, Router};
pub use web_component::*;

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

pub type MinfacRouter = pilatus::OnceExtractor<axum::Router>;

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
