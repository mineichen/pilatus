use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use axum::routing::get_service;
use futures::{channel::oneshot, FutureExt};
use minfac::{AllRegistered, Registered, ServiceCollection, WeakServiceProvider};
use pilatus::{prelude::*, GenericConfig, OnceExtractor, SystemShutdown, TracingTopic};
use pilatus_axum::MinfacRouter;
use serde::Deserialize;
use tokio::net::TcpListener;
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::{debug, info};

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<(
        (WeakServiceProvider, Registered<PrivateRouter>),
        Registered<GenericConfig>,
        Registered<SystemShutdown>,
        Registered<Arc<PrivateState>>,
    )>()
    .register_hosted_service("Main Webserver", axum_service);
    c.register(|| TracingTopic::new("tower_http", tracing::Level::INFO));
    c.register(|| TracingTopic::new("tungstenite::protocol", tracing::Level::INFO));
    c.with::<(
        AllRegistered<MinfacRouter>,
        AllRegistered<Box<dyn FnOnce(axum::Router) -> axum::Router>>,
    )>()
    .register(|(routes, raw)| {
        PrivateRouter(raw.fold(axum::Router::new(), |acc, r| r(acc)).nest(
            "/api",
            routes.fold(axum::Router::new(), |acc, n| {
                acc.merge(n.extract_unchecked())
            }),
        ))
    });
    c.register_shared(|| {
        let (tx, rx) = oneshot::channel();
        Arc::new(PrivateState(tx.into(), rx.shared()))
    })
    .alias(|s| pilatus_axum::Stats::new(s.1.clone()));
}

struct PrivateRouter(axum::Router);

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
struct WebConfig {
    socket: SocketAddr,
    body_limit: usize,
    fallback_path: Option<PathBuf>,
}

struct PrivateState(
    OnceExtractor<oneshot::Sender<SocketAddr>>,
    futures::future::Shared<oneshot::Receiver<SocketAddr>>,
);

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            socket: SocketAddr::from(([0, 0, 0, 0], 80)),
            body_limit: 8 * 1024 * 1024,
            fallback_path: None,
        }
    }
}

async fn axum_service(
    ((provider, private_router), config, shutdown, private_state): (
        (WeakServiceProvider, PrivateRouter),
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
        "Starting axum on port {} {}",
        web_config.socket,
        if let Some(x) = &web_config.fallback_path {
            format!("with static files on path {x:?}")
        } else {
            "without fallback to static files".to_string()
        }
    );

    let listener = TcpListener::bind(&web_config.socket)
        .await
        .context("Cannot open TCP-Connection for webserver. Is pilatus running already?")?;
    private_state
        .0
        .extract_unchecked()
        .send(listener.local_addr()?)
        .expect("Receiver is stored within DI-Container");

    let router = if let Some(x) = &web_config.fallback_path {
        private_router
            .0
            .fallback_service(get_service(ServeDir::new(x)))
    } else {
        private_router.0
    };
    let router = router
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
        assert_eq!(adr.fallback_path, None);
    }
}
