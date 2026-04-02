use std::marker::PhantomData;

use futures::{
    channel::{mpsc, oneshot},
    future::Either,
    stream::BoxStream,
    Future, SinkExt, StreamExt,
};
use pilatus::device::{ActorError, ActorMessage, ActorSystem, DynamicIdentifier};
use pilatus_axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::StatusCode,
    IntoResponse,
};
use tracing::debug;

use crate::image::protocol::StreamableImage;

use super::{
    BroadcastImage, LocalizableBroadcastImage, SubscribeImageMessage, SubscribeImageOk,
    SubscribeLocalizableImageMessage, SubscribeLocalizableImageOk,
};

pub type DefaultImageStreamer =
    ImageStreamer<SubscribeImageMessage, SubscribeImageOk, BroadcastImage>;

pub type LocalizableImageStreamer = ImageStreamer<
    SubscribeLocalizableImageMessage,
    SubscribeLocalizableImageOk,
    LocalizableBroadcastImage,
>;

pub struct ImageStreamer<TMsg, TInputStream, TInputImage>(
    PhantomData<(TMsg, TInputStream, TInputImage)>,
);
impl<TMsg, TInputStream, TInputImage> ImageStreamer<TMsg, TInputStream, TInputImage> {}

impl<TMsg, TInputStream, TInputImage> ImageStreamer<TMsg, TInputStream, TInputImage>
where
    TMsg: Default + ActorMessage<Output = TInputStream>,
    TInputImage: Clone + Send + 'static,
    TInputStream: Into<BoxStream<'static, TInputImage>>,
{
    pub async fn stream_image<
        TImg: StreamableImage + Send + 'static,
        TFn: Fn(TInputImage) -> TFut + 'static + Send + Sync,
        TFut: Future<Output = Result<TImg, ActorError<anyhow::Error>>> + 'static + Send,
    >(
        upgrade: WebSocketUpgrade,
        device_id: DynamicIdentifier,
        actor_system: ActorSystem,
        transformer: TFn,
    ) -> Result<impl IntoResponse, (StatusCode, String)> {
        Self::bidirectional_stream_image(upgrade, device_id, actor_system, transformer, |_| async {
            Ok(())
        })
        .await
    }
    pub async fn bidirectional_stream_image<
        TImg: StreamableImage + Send + 'static,
        TFn: Fn(TInputImage) -> TFut + 'static + Send + Sync,
        TFut: Future<Output = Result<TImg, ActorError<anyhow::Error>>> + 'static + Send,
        TMessageHandler: (Fn(Message) -> TMessageHandlerFuture) + Send + Sync + 'static,
        TMessageHandlerFuture: Future<Output = Result<(), anyhow::Error>> + 'static + Send,
    >(
        upgrade: WebSocketUpgrade,
        device_id: DynamicIdentifier,
        actor_system: ActorSystem,
        transformer: TFn,
        message_handler: TMessageHandler,
    ) -> Result<impl IntoResponse, (StatusCode, String)> {
        Self::try_bidirectional_stream_image(
            upgrade,
            device_id,
            actor_system,
            transformer,
            message_handler,
        )
        .await
        .map_err(|(_, r)| r)
    }
    pub async fn try_bidirectional_stream_image<
        TImg: StreamableImage + Send + 'static,
        TFn: Fn(TInputImage) -> TFut + 'static + Send + Sync,
        TFut: Future<Output = Result<TImg, ActorError<anyhow::Error>>> + 'static + Send,
        TMessageHandler: (Fn(Message) -> TMessageHandlerFuture) + Send + Sync + 'static,
        TMessageHandlerFuture: Future<Output = Result<(), anyhow::Error>> + 'static + Send,
    >(
        upgrade: WebSocketUpgrade,
        device_id: DynamicIdentifier,
        actor_system: ActorSystem,
        transformer: TFn,
        message_handler: TMessageHandler,
    ) -> Result<impl IntoResponse, (WebSocketUpgrade, (StatusCode, String))> {
        let broadcast = {
            let mut sender = match actor_system.get_sender::<TMsg>(device_id) {
                Ok(x) => x,
                Err(e) => return Err((upgrade, (StatusCode::NOT_FOUND, e.to_string()))),
            };
            match sender.ask(TMsg::default()).await {
                Ok(x) => x,
                Err(e) => return Err((upgrade, (StatusCode::NOT_FOUND, e.to_string()))),
            }
        }
        .into();
        Ok(upgrade.on_upgrade(move |socket| async move {
            Self::handle_socket(socket, broadcast, transformer, message_handler).await;
            debug!("Websocket subscription ended");
        }))
    }

    async fn handle_socket<
        TImg: StreamableImage + Send + 'static,
        TFn: Fn(TInputImage) -> TFut + 'static + Send,
        TFut: Future<Output = Result<TImg, ActorError<anyhow::Error>>> + 'static + Send,
        TMessageHandler: (Fn(Message) -> TMessageHandlerFuture) + Send + 'static,
        TMessageHandlerFuture: Future<Output = Result<(), anyhow::Error>> + 'static + Send,
    >(
        socket: WebSocket,
        mut broadcast: BoxStream<'static, TInputImage>,
        transformer: TFn,
        message_handler: TMessageHandler,
    ) {
        let (mut socket_tx, mut socket_rx) = socket.split();
        let (signal_broadcast_end, mut receive_broadcast_end) = oneshot::channel();
        let (mut tx, rx) = mpsc::channel(10);
        let encode_task = async move {
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
                if socket_tx.send(Message::Binary(x.into())).await.is_err() {
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
}
