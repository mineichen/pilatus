use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use axum::routing::get_service;
use futures::{channel::oneshot, FutureExt};
use minfac::{Registered, ServiceCollection, WeakServiceProvider};
use pilatus::{prelude::*, GenericConfig, OnceExtractor, SystemShutdown, TracingTopic};
use pilatus_axum::MinfacRouter;
use serde::Deserialize;
use tokio::net::TcpListener;
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::{debug, info};

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<(
        WeakServiceProvider,
        Registered<GenericConfig>,
        Registered<SystemShutdown>,
        Registered<Arc<PrivateState>>,
    )>()
    .register_hosted_service("Main Webserver", axum_service);
    c.register(|| TracingTopic::new("tower_http", tracing::Level::INFO));
    c.register(|| TracingTopic::new("tungstenite::protocol", tracing::Level::INFO));
    c.register_shared(|| {
        let (tx, rx) = oneshot::channel();
        Arc::new(PrivateState(tx.into(), rx.shared()))
    })
    .alias(|s| pilatus_axum::Stats::new(s.1.clone()));
}

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
struct WebConfig {
    socket: SocketAddr,
    frontend: PathBuf,
    body_limit: usize,
}

struct PrivateState(
    OnceExtractor<oneshot::Sender<SocketAddr>>,
    futures::future::Shared<oneshot::Receiver<SocketAddr>>,
);

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            socket: SocketAddr::from(([0, 0, 0, 0], 80)),
            frontend: "dist".into(),
            body_limit: 8 * 1024 * 1024,
        }
    }
}

async fn axum_service(
    (provider, config, shutdown, private_state): (
        WeakServiceProvider,
        GenericConfig,
        SystemShutdown,
        Arc<PrivateState>,
    ),
) -> Result<(), anyhow::Error> {
    let web_config = config.get_or_default::<WebConfig>("web");
    debug!(
        "WebConfig: {}, raw: {:?}",
        serde_json::to_string(&web_config).unwrap(),
        &config
    );
    info!(
        "Starting axum on port {} with frontend on path {:?}",
        web_config.socket, web_config.frontend
    );

    let listener = TcpListener::bind(&web_config.socket)
        .await
        .context("Cannot open TCP-Connection for webserver. Is pilatus running already?")?;
    private_state
        .0
        .extract_unchecked()
        .send(listener.local_addr()?)
        .expect("Receiver is stored within DI-Container");

    let router = axum::Router::new()
        .nest(
            "/api",
            provider
                .get_all::<MinfacRouter>()
                .fold(axum::Router::new(), |acc, n| {
                    acc.merge(n.extract_unchecked())
                }),
        )
        .fallback_service(get_service(ServeDir::new(web_config.frontend)))
        .layer(super::inject::InjectLayer(provider))
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        .layer(axum::extract::DefaultBodyLimit::max(web_config.body_limit))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .into_make_service();
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            shutdown.await;
            debug!("Shutdown is triggered");
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_wildcard() {
        let raw = r#"{
            "socket": "0.0.0.0:20"
        }"#;
        let adr: WebConfig = serde_json::from_str(raw).unwrap();
        assert_eq!(adr.socket.ip().to_string(), "0.0.0.0");
        assert_eq!(adr.frontend, WebConfig::default().frontend);
    }
}
