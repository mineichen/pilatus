/// # Reason for introducing new protocol
/// The initial protocoll stopped if a error occured. Such errors could be temporal (camera not found) or fixable by changing parameters back)
/// In that case, the subscriber started to randomly request new frames without knowing if this is reasonable
/// The new design still allows all previous workflows by simply adding .take_while() and therefore volunatarely close the stream.
/// Furthermore, the new design allows errors to contain images, for situations, where e.g.
use std::{
    fmt::{self, Debug, Formatter},
    marker::PhantomData,
    num::NonZeroU32,
    sync::Arc,
};

use anyhow::anyhow;
use bytes::BufMut;
use futures::{
    channel::{mpsc, oneshot},
    future::Either,
    stream::BoxStream,
    Future, SinkExt, StreamExt,
};
use jpeg_encoder::{ColorType, Encoder};
use pilatus::device::{ActorError, ActorMessage, ActorSystem, DeviceId};
use pilatus_engineering::image::{
    BroadcastImage, DynamicImage, ImageWithMeta, LocalizableBroadcastImage, LumaImage,
    PackedGenericImage, RgbImage, StreamImageError, SubscribeImageMessage, SubscribeImageOk,
    SubscribeLocalizableImageMessage, SubscribeLocalizableImageOk, UnpackedGenericImage,
};
use serde::Serialize;
use tracing::{debug, trace};

use crate::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::StatusCode,
    IntoResponse,
};
pub trait StreamableImage: Sized {
    fn encode(self) -> anyhow::Result<Vec<u8>>;
}

impl StreamableImage for Arc<LumaImage> {
    fn encode(self) -> anyhow::Result<Vec<u8>> {
        let dims = self.dimensions();
        encode_legacy(self.buffer(), ColorType::Luma, dims, |_| Ok(()))
    }
}

const OK_CODE: u8 = 0 << 4;
const MISSED_ITEM_CODE: u8 = 1 << 4;
const PROCESSING_CODE: u8 = 2 << 4;
const ACTOR_ERROR_CODE: u8 = 3 << 4;

#[derive(Default, serde::Deserialize, Clone, Copy)]
pub enum StreamingImageFormat {
    #[default]
    Jpeg,
    Raw,
}

/// Protocol Spec
///                   | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
/// 0..1              | ok/err codes  |    reserved   |
/// 1..4              |           reserved            |
/// 4..8              |   u32::LE_bytes of MetaLen    |
/// 8..(MetaLen + 8)  |          Meta as JSON         |
///                   |   empty for image alignment   |
/// (MetaLen+8)..(+12)| u32::LE_bytes of MainImagSize |
/// omitted here      |       encoded MainImage       |
///
/// foreach addidtional image (ordering defined by request)
///                   |    u32::LE_bytes of ImageSize |
///                   |         encoded Image         |
///
impl StreamableImage
    for (
        Result<ImageWithMeta<DynamicImage>, StreamImageError<DynamicImage>>,
        StreamingImageFormat,
    )
{
    fn encode(self) -> anyhow::Result<Vec<u8>> {
        match self.0 {
            Ok(x) => self.1.encode_dynamic_image(OK_CODE, x.image, x.meta),
            Err(e) => match e {
                StreamImageError::MissedItems(_) => {
                    encode_meta(vec![MISSED_ITEM_CODE, 0, 0, 0], |_| Ok(()))
                }
                StreamImageError::ProcessingError { image, error } => {
                    debug!("Processing error: {error}");
                    self.1
                        .encode_dynamic_image(PROCESSING_CODE, image, error.to_string())
                }
                StreamImageError::ActorError(_) => {
                    encode_meta(vec![ACTOR_ERROR_CODE, 0, 0, 0], |_| Ok(()))
                }
                _ => Err(anyhow::anyhow!("Unknown error: {e:?}")),
            },
        }
    }
}

impl StreamingImageFormat {
    fn encode_dynamic_image<T: Serialize>(
        self,
        code: u8,
        image: DynamicImage,
        meta: T,
    ) -> anyhow::Result<Vec<u8>> {
        match self {
            StreamingImageFormat::Jpeg => encode_dynamic_jpeg_image(code, image, meta),
            StreamingImageFormat::Raw => encode_dynamic_raw_image(code, image, meta),
        }
    }
}

fn prepare_dynamic_image_buf<T: Serialize>(
    flag: u8,
    meta: T,
    capacity: usize,
) -> anyhow::Result<Vec<u8>> {
    let meta_writer = move |b: &mut Vec<u8>| serde_json::to_writer(b, &meta).map_err(Into::into);
    let mut buf = Vec::with_capacity(capacity);
    buf.extend_from_slice(&[flag, 0, 0, 0]);
    encode_meta(buf, meta_writer)
}

fn encode_dynamic_raw_image<T: Serialize>(
    flag: u8,
    image: DynamicImage,
    meta: T,
) -> anyhow::Result<Vec<u8>> {
    let dims = image.dimensions();
    let (width, height) = dims;
    let buf =
        prepare_dynamic_image_buf(flag, meta, width.get() as usize * height.get() as usize / 2)?;
    match image {
        DynamicImage::Luma8(i) => encode_raw(buf, i.buffer(), DataType::U8, 1, dims),
        DynamicImage::Luma16(i) => {
            encode_raw(buf, bytes_from_u16(i.buffer())?, DataType::U16, 1, dims)
        }
        DynamicImage::Rgb8Planar(i) => encode_raw(buf, i.buffer(), DataType::U8, 3, dims),
        _ => Err(anyhow!("Unsupported image format: {:?}", image)),
    }
}

fn bytes_from_u16(from: &[u16]) -> anyhow::Result<&[u8]> {
    if cfg!(target_endian = "big") {
        return Err(anyhow::anyhow!("Not implemented on big endian platforms"));
    }

    let len = from.len().checked_mul(2).unwrap();
    let ptr: *const u8 = from.as_ptr().cast();
    Ok(unsafe { std::slice::from_raw_parts(ptr, len) })
}

fn encode_dynamic_jpeg_image<T: Serialize>(
    flag: u8,
    image: DynamicImage,
    meta: T,
) -> anyhow::Result<Vec<u8>> {
    let dims = image.dimensions();
    let buf = prepare_dynamic_image_buf(
        flag,
        meta,
        dims.0.get() as usize * dims.1.get() as usize / 2,
    )?;
    match image {
        DynamicImage::Luma8(i) => encode_jpeg(buf, i.buffer(), ColorType::Luma, dims),
        DynamicImage::Luma16(i) => encode_jpeg(
            buf,
            &i.buffer()
                .iter()
                .map(|x| (x >> 8) as u8)
                .collect::<Vec<_>>(),
            ColorType::Luma,
            dims,
        ),
        DynamicImage::Rgb8Planar(i) => {
            let packed: PackedGenericImage = UnpackedGenericImage::new(i).into();
            encode_jpeg(buf, packed.buffer(), ColorType::Rgb, dims)
        }
        _ => Err(anyhow!("Unsupported image format: {:?}", image)),
    }
}

impl<T: Serialize> StreamableImage for (Arc<LumaImage>, T) {
    fn encode(self) -> anyhow::Result<Vec<u8>> {
        let dims = self.0.dimensions();
        encode_legacy(self.0.buffer(), ColorType::Luma, dims, |b| {
            serde_json::to_writer(b, &self.1).map_err(Into::into)
        })
    }
}

pub struct RgbImageWithMetadata<T>(pub Arc<dyn RgbImage + Send + Sync>, pub T);

impl<T> RgbImageWithMetadata<T> {
    pub fn new(img: Arc<dyn RgbImage + Send + Sync>, meta: T) -> Self {
        Self(img, meta)
    }
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
    fn encode(self) -> anyhow::Result<Vec<u8>> {
        let dims = self.0.size();
        let packed = self.0.into_packed();
        encode_legacy(packed.buffer(), ColorType::Rgb, dims, |b| {
            serde_json::to_writer(b, &self.1).map_err(Into::into)
        })
    }
}

fn encode_legacy(
    image: &[u8],
    color: ColorType,
    (width, height): (NonZeroU32, NonZeroU32),
    meta: impl FnOnce(&mut Vec<u8>) -> anyhow::Result<()>,
) -> anyhow::Result<Vec<u8>> {
    let target = Vec::with_capacity(width.get() as usize * height.get() as usize);
    let (width, height) = (width, height);
    let mut buf = encode_meta(target, meta)?;
    let encoder = Encoder::new(&mut buf, 80);
    let t = std::time::Instant::now();
    encoder.encode(image, width.get() as u16, height.get() as u16, color)?;
    trace!("encoding time: {}ms", t.elapsed().as_millis());
    Ok(buf)
}

fn encode_jpeg(
    mut buf: Vec<u8>,
    image: &[u8],
    color: ColorType,
    (width, height): (NonZeroU32, NonZeroU32),
) -> anyhow::Result<Vec<u8>> {
    buf.extend_from_slice(&[0, 0, 0, 0]);
    let offset = buf.len();
    let encoder = Encoder::new(&mut buf, 80);
    let t = std::time::Instant::now();
    encoder.encode(image, width.get() as u16, height.get() as u16, color)?;
    trace!("encoding time: {}ms", t.elapsed().as_millis());
    let size = (buf.len() - offset) as u32;
    buf[offset - 4..offset].copy_from_slice(&size.to_le_bytes());
    Ok(buf)
}

#[repr(u8)]
enum DataType {
    U8,
    U16,
}

fn encode_raw(
    mut buf: Vec<u8>,
    image: &[u8],
    pixel_kind: DataType,
    channels: u16,
    (width, height): (NonZeroU32, NonZeroU32),
) -> anyhow::Result<Vec<u8>> {
    // https://stackoverflow.com/questions/45213511/formula-for-memory-alignment
    let unaligned_pixel_start = buf.len() + 4;
    let alignment_bytes = (((unaligned_pixel_start + 7) & !7) - unaligned_pixel_start) as u32;

    const HEADER_BYTE_SIZE: u32 = 8;
    buf.extend_from_slice(&(image.len() as u32 + HEADER_BYTE_SIZE + alignment_bytes).to_le_bytes());

    buf.extend((0..alignment_bytes).map(|_| 0)); // Guarantee 8Byte aligned
    buf.push(0u8); // reserved
    buf.push(pixel_kind as u8);
    buf.put_slice(&channels.to_le_bytes());
    buf.put_slice(&width.get().to_le_bytes());
    buf.put_slice(image);
    trace!(
        "Encoded raw: {:?}, width: {width}, height: {height}",
        &buf[0..buf.len().min(10)]
    );
    Ok(buf)
}

fn encode_meta(
    mut buf: Vec<u8>,
    meta: impl FnOnce(&mut Vec<u8>) -> anyhow::Result<()>,
) -> anyhow::Result<Vec<u8>> {
    let offset = buf.len();
    buf.extend_from_slice(&[0, 0, 0, 0]);
    (meta)(&mut buf)?;
    let meta_length = (buf.len() as u32 - (4 + offset as u32)).to_le_bytes();
    buf[offset..(offset + 4)].copy_from_slice(&meta_length);
    Ok(buf)
}

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
    TInputImage: Clone + Send + Sync + 'static,
    TInputStream: Into<BoxStream<'static, TInputImage>>,
{
    pub async fn stream_image<
        TImg: StreamableImage + Send + Sync + 'static,
        TFn: Fn(TInputImage) -> TFut + 'static + Send + Sync,
        TFut: Future<Output = Result<TImg, ActorError<anyhow::Error>>> + 'static + Send,
    >(
        upgrade: WebSocketUpgrade,
        device_id: Option<DeviceId>,
        actor_system: ActorSystem,
        transformer: TFn,
    ) -> Result<impl IntoResponse, (StatusCode, String)> {
        Self::bidirectional_stream_image(upgrade, device_id, actor_system, transformer, |_| async {
            Ok(())
        })
        .await
    }
    pub async fn bidirectional_stream_image<
        TImg: StreamableImage + Send + Sync + 'static,
        TFn: Fn(TInputImage) -> TFut + 'static + Send + Sync,
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
        TImg: StreamableImage + Send + Sync + 'static,
        TFn: Fn(TInputImage) -> TFut + 'static + Send + Sync,
        TFut: Future<Output = Result<TImg, ActorError<anyhow::Error>>> + 'static + Send,
        TMessageHandler: (Fn(Message) -> TMessageHandlerFuture) + Send + Sync + 'static,
        TMessageHandlerFuture: Future<Output = Result<(), anyhow::Error>> + 'static + Send,
    >(
        upgrade: WebSocketUpgrade,
        device_id: Option<DeviceId>,
        actor_system: ActorSystem,
        transformer: TFn,
        message_handler: TMessageHandler,
    ) -> Result<impl IntoResponse, (WebSocketUpgrade, (StatusCode, String))> {
        let broadcast = {
            let mut sender = match actor_system.get_sender_or_single_handler::<TMsg>(device_id) {
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
        TImg: StreamableImage + Send + Sync + 'static,
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
