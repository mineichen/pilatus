use futures::{channel::mpsc::Receiver, future::Either, StreamExt};
use minfac::ServiceCollection;
use pilatus::device::ActorSystem;
use pilatus_axum::{
    extract::{
        ws::{WebSocket, WebSocketUpgrade},
        InjectRegistered,
    },
    http::StatusCode,
    IntoResponse, ServiceCollectionExtensions,
};
use tracing::debug;

use crate::producer::{SubscribeMessage, Heartbeat};

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.register_web("coinbase", |b| b.http("", |f| f.get(stream_coinbase)));
}

async fn stream_coinbase(
    upgrade: WebSocketUpgrade,
    InjectRegistered(system): InjectRegistered<ActorSystem>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let x = system
        .get_sender_or_single_handler(None)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
        .ask(SubscribeMessage::<Heartbeat>::new("BTC-USD"))
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(upgrade.on_upgrade(move |s| async move {
        if let Err(e) = handle_socket(s, x).await {
            eprintln!("Abort stream: {e}")
        }
    }))
}

async fn handle_socket(
    mut consumer: WebSocket,
    mut coinbase_data: Receiver<Heartbeat>,
) -> Result<(), anyhow::Error> {
    debug!("Open websocket");
    while let Either::Right((Some(data), _)) =
        futures::future::select(consumer.next(), coinbase_data.next()).await
    {
        let json = serde_json::to_string(&data)?;
        consumer
            .send(pilatus_axum::extract::ws::Message::Text(json))
            .await?;
    }
    coinbase_data.close();
    debug!("Closed websocket");
    Ok(())
}
