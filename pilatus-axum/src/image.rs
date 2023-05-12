use std::{
    fmt::{self, Debug, Formatter},
    sync::Arc,
};

use futures::{
    channel::{mpsc, oneshot},
    future::Either,
    Future, SinkExt, Stream, StreamExt,
};
use jpeg_encoder::{ColorType, Encoder};
use pilatus::device::{ActorError, ActorMessage, ActorSystem, DeviceId};
use pilatus_engineering::image::{BroadcastImage, LumaImage, RgbImage, SubscribeImageMessage};
use serde::Serialize;
use tracing::{debug, trace};

use crate::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::StatusCode,
    IntoResponse,
};
pub trait StreamableImage: Sized {
    type InputImage: Clone + Send + Sync;
    type CameraMessage: Default
        + ActorMessage<
            Output = Box<dyn futures::stream::Stream<Item = Self::InputImage> + Send + Sync>,
        >;
    fn encode(self) -> anyhow::Result<Vec<u8>>;
}

impl StreamableImage for Arc<LumaImage> {
    type InputImage = BroadcastImage;
    type CameraMessage = SubscribeImageMessage;
    fn encode(self) -> anyhow::Result<Vec<u8>> {
        let dims = self.dimensions();
        encode(self.buffer(), ColorType::Luma, dims, |_| Ok(()))
    }
}

impl<T: Serialize> StreamableImage for (Arc<LumaImage>, T) {
    type InputImage = BroadcastImage;
    type CameraMessage = SubscribeImageMessage;
    fn encode(self) -> anyhow::Result<Vec<u8>> {
        let dims = self.0.dimensions();
        encode(self.0.buffer(), ColorType::Luma, dims, |b| {
            serde_json::to_writer(b, &self.1).map_err(Into::into)
        })
    }
}

pub struct RgbImageWithMetadata<T>(pub Arc<dyn RgbImage + Send + Sync>, pub T);

impl<T> RgbImageWithMetadata<T> {
    pub fn get_meta(&self) -> &T {
        &self.1
    }
}

impl<T: Debug> Debug for RgbImageWithMetadata<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RgbImageWithMetadata")
            .field(&self.1)
            .finish()
    }
}

impl<T: Serialize> StreamableImage for RgbImageWithMetadata<T> {
    type InputImage = BroadcastImage;
    type CameraMessage = SubscribeImageMessage;

    fn encode(self) -> anyhow::Result<Vec<u8>> {
        let dims = self.0.size();
        let packed = self.0.into_packed();
        encode(packed.buffer(), ColorType::Rgb, dims, |b| {
            serde_json::to_writer(b, &self.1).map_err(Into::into)
        })
    }
}

fn encode(
    image: &[u8],
    color: ColorType,
    (width, height): (u32, u32),
    meta: impl FnOnce(&mut Vec<u8>) -> anyhow::Result<()>,
) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity((width * height) as usize);
    buf.extend_from_slice(&[0, 0, 0, 0]);
    (meta)(&mut buf)?;
    let meta_length = (buf.len() as u32 - 4).to_le_bytes();
    buf[0..4].copy_from_slice(&meta_length);

    let encoder = Encoder::new(&mut buf, 80);
    let t = std::time::Instant::now();
    encoder.encode(image, width as u16, height as u16, color)?;
    trace!("encoding time: {}ms", t.elapsed().as_millis());
    Ok(buf)
}

pub async fn stream_image<
    TImg: StreamableImage + Send + Sync + 'static,
    TFn: Fn(TImg::InputImage) -> TFut + 'static + Send + Sync,
    TFut: Future<Output = Result<TImg, ActorError<anyhow::Error>>> + 'static + Send,
>(
    upgrade: WebSocketUpgrade,
    device_id: Option<DeviceId>,
    actor_system: ActorSystem,
    transformer: TFn,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    bidirectional_stream_image(upgrade, device_id, actor_system, transformer, |_| async {
        Ok(())
    })
    .await
}

// device_id corresponds to the camera the websocket subscribes to via (SubscribeImageMessage)
pub async fn bidirectional_stream_image<
    TImg: StreamableImage + Send + Sync + 'static,
    TFn: Fn(TImg::InputImage) -> TFut + 'static + Send + Sync,
    TFut: Future<Output = Result<TImg, ActorError<anyhow::Error>>> + 'static + Send,
    TMessageHandler: (Fn(Message) -> TMessageHandlerFuture) + Send + Sync + 'static,
    TMessageHandlerFuture: Future<Output = Result<(), anyhow::Error>> + 'static + Send,
>(
    upgrade: WebSocketUpgrade,
    device_id: Option<DeviceId>,
    actor_system: ActorSystem,
    transformer: TFn,
    message_handler: TMessageHandler,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut sender = actor_system
        .get_sender_or_single_handler::<TImg::CameraMessage>(device_id)
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    let broadcast = sender
        .ask(TImg::CameraMessage::default())
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(upgrade.on_upgrade(move |socket| async move {
        handle_socket::<TImg, TFn, TFut, TMessageHandler, TMessageHandlerFuture>(
            socket,
            broadcast,
            transformer,
            message_handler,
        )
        .await;
        debug!("Websocket subscription ended");
    }))
}

async fn handle_socket<
    TImg: StreamableImage + Send + Sync + 'static,
    TFn: Fn(TImg::InputImage) -> TFut + 'static + Send,
    TFut: Future<Output = Result<TImg, ActorError<anyhow::Error>>> + 'static + Send,
    TMessageHandler: (Fn(Message) -> TMessageHandlerFuture) + Send + 'static,
    TMessageHandlerFuture: Future<Output = Result<(), anyhow::Error>> + 'static + Send,
>(
    socket: WebSocket,
    broadcast: Box<dyn Stream<Item = TImg::InputImage> + Send + Sync>,
    transformer: TFn,
    message_handler: TMessageHandler,
) {
    let (mut socket_tx, mut socket_rx) = socket.split();
    let (signal_broadcast_end, mut receive_broadcast_end) = oneshot::channel();
    let (mut tx, rx) = mpsc::channel(10);
    let encode_task = async move {
        let mut broadcast = Box::into_pin(broadcast);

        while let Some(image) = broadcast.next().await {
            let image = (transformer)(image).await?;
            let encoded_image = pilatus::execute_blocking(move || image.encode()).await?;
            tx.send(encoded_image).await?;
        }
        debug!("Close connection because broadcast is closed");
        let _ignore = signal_broadcast_end.send(());
        Ok(()) as anyhow::Result<()>
    };
    let send_task = async move {
        // Without move, encode_task doesn't stop
        let mut moved_rx: mpsc::Receiver<_> = rx;
        while let Some(x) = moved_rx.next().await {
            if socket_tx.send(Message::Binary(x)).await.is_err() {
                break;
            }
        }
        debug!("Websocket sender finished");
    };
    let read_task = async move {
        while let Either::Right((Some(Ok(msg)), _)) =
            futures::future::select(&mut receive_broadcast_end, socket_rx.next()).await
        {
            if (message_handler)(msg).await.is_err() {
                break;
            }
        }
    };

    let _ = futures::join!(encode_task, send_task, read_task);
}
