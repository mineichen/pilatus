/// # Reason for introducing new protocol
/// The initial protocoll stopped if a error occured. Such errors could be temporal (camera not found) or fixable by changing parameters back)
/// In that case, the subscriber started to randomly request new frames without knowing if this is reasonable
/// The new design still allows all previous workflows by simply adding .take_while() and therefore volunatarely close the stream.
/// Furthermore, the new design allows errors to contain images, for situations, where e.g.
use std::{
    collections::HashMap,
    fmt::{self, Debug, Formatter},
    io::Write,
    num::{NonZeroU16, NonZeroU32, NonZeroU8},
    sync::Arc,
};

use anyhow::{anyhow, Context};
use bytes::BufMut;
use imbuf::{DynamicImage, DynamicSize, ImageChannel, PixelTypePrimitive};
use pilatus::MissedItemsError;
use serde::Serialize;
use tracing::{debug, trace};

use crate::image::{
    protocol::{calculate_buf_len, into_extensions_map},
    AnyMultiMap, DataType,
};

use super::{
    super::{ImageWithMeta, LumaImage, Rgb8Image, StreamImageError},
    StreamableImage, CODE_ACTOR_ERROR, CODE_MISSED_ITEM, CODE_OK, CODE_PROCESSING,
};

type EncodeExtensionWriter =
    Box<dyn Fn(&AnyMultiMap, &mut Vec<u8>) -> std::io::Result<()> + Send + Sync>;
pub struct MetaEncodeExtension {
    pub kind: u8,
    pub writer: EncodeExtensionWriter,
}
#[derive(Default)]
pub struct MetaEncodeExtensions(HashMap<u8, EncodeExtensionWriter>);

impl MetaEncodeExtensions {
    pub fn new(extensions: impl IntoIterator<Item = MetaEncodeExtension>) -> Self {
        let iter = extensions.into_iter().map(|x| (x.kind, x.writer));
        Self(into_extensions_map(iter))
    }
}
impl StreamableImage for LumaImage {
    fn encode(self, encoder: &MetaImageEncoder) -> anyhow::Result<Vec<u8>> {
        let meta = ImageWithMeta::with_hash(self.into(), None);
        encoder.encode(Ok(meta))
    }
}

/// Protocol Spec
/// Reserved have to be written as 0, which is debug_checked by decoders
///
///|||||||||||||||||||| 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
/// 0..1              | ok/err codes  |    reserved   |
/// 1..2              |            version            |
/// 2..4              |            reserved           |
/// 4..8              |   u32::LE_bytes of MetaLen    |
/// 8..(MetaLen + 8)  |          Meta as JSON         |
///
/// Chunks...
/// 0..1              |     kind      |   reserved    |
/// 1..               |   data (kind determines end)  |
///
/// Chunks - RawImage (kind = 0)
///
///
/// (There is currently no time to build a clean Zip-File with adequate file formats
/// - BigTiff instead of rawImage has async rust encoders/decoders for uncompressed data (nocopy on client side)
/// - Streaming data (Size not known when write starts) could drastically reduce memory footprint
#[derive(Clone)]
pub struct MetaImageEncoder {
    extensions: Arc<MetaEncodeExtensions>,
}

impl MetaImageEncoder {
    pub fn with_extensions(extensions: Arc<MetaEncodeExtensions>) -> Self {
        Self { extensions }
    }

    pub fn encode(
        &self,
        image: Result<ImageWithMeta<DynamicImage>, StreamImageError<DynamicImage>>,
    ) -> anyhow::Result<Vec<u8>> {
        match image {
            Ok(x) => encode_dynamic_image(CODE_OK, x.image, x.extensions, x.meta, &self.extensions),
            Err(e) => match e {
                #[expect(deprecated)]
                StreamImageError::MissedItems(MissedItemsError { number, .. }) => {
                    encode_meta(prepare_buffer(CODE_MISSED_ITEM, 12), |x| {
                        Ok(x.write_all(&number.0.to_le_bytes())?)
                    })
                }
                StreamImageError::ProcessingError { image, error } => {
                    debug!("Processing error: {error}");
                    encode_dynamic_image(
                        CODE_PROCESSING,
                        image,
                        Default::default(),
                        error.to_string(),
                        &self.extensions,
                    )
                }
                StreamImageError::ActorError(_) => {
                    encode_meta(prepare_buffer(CODE_ACTOR_ERROR, 4), |_| Ok(()))
                }
                _ => Err(anyhow::anyhow!("Unknown error: {e:?}")),
            },
        }
    }
}

impl StreamableImage for Result<ImageWithMeta<DynamicImage>, StreamImageError<DynamicImage>> {
    fn encode(self, encoder: &MetaImageEncoder) -> anyhow::Result<Vec<u8>> {
        encoder.encode(self)
    }
}

fn prepare_buffer(flag: u16, capacity: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(capacity);
    buf.extend_from_slice(&flag.to_le_bytes());
    buf.push(0);
    buf.push(0);
    buf
}

fn prepare_dynamic_image_buf<T: Serialize>(
    flag: u16,
    meta: T,
    capacity: usize,
) -> anyhow::Result<Vec<u8>> {
    let buf = prepare_buffer(flag, capacity);
    let meta_writer = move |b: &mut Vec<u8>| serde_json::to_writer(b, &meta).map_err(Into::into);
    encode_meta(buf, meta_writer)
}

fn encode_dynamic_image<T: Serialize>(
    flag: u16,
    image: DynamicImage,
    extensions: AnyMultiMap,
    meta: T,
    write_extensions: &MetaEncodeExtensions,
) -> anyhow::Result<Vec<u8>> {
    fn encode_typed<TPixel: PixelTypePrimitive>(
        x: &ImageChannel<DynamicSize<TPixel>>,
        buf: Vec<u8>,
        pixel_kind: DataType,
    ) -> anyhow::Result<Vec<u8>> {
        encode_raw(
            buf,
            x.buffer_flat_bytes(),
            pixel_kind,
            x.dimensions(),
            x.pixel_elements(),
        )
    }
    let image_len = image.iter().try_fold(0, |acc, ch| {
        let header = usize::from(super::CHANNEL_HEADER_BYTE_SIZE);
        anyhow::Ok(acc + header + calculate_buf_len(ch.dimensions(), ch.pixel_elements())?)
    })? + 2;
    let mut buf = prepare_dynamic_image_buf(flag, meta, 0)?;

    let capacity = buf.len() + image_len;
    buf.reserve(capacity);
    let channels =
        NonZeroU16::try_from(image.len_nonzero()).context("Number of Channels is too big")?;
    buf.put_slice(&channels.get().to_le_bytes());
    for channel in image.iter() {
        buf = match channel {
            imbuf::DynamicImageChannel::U8(x) => encode_typed(x, buf, DataType::U8),
            imbuf::DynamicImageChannel::U16(x) => encode_typed(x, buf, DataType::U16),
            _ => Err(anyhow!("Unsupported image format: {:?}", image)),
        }?;
    }

    debug_assert_eq!(
        buf.len(),
        capacity,
        "Reserve-Exact should only be used if capacity is correct"
    );

    for (_kind, ext) in write_extensions.0.iter() {
        (ext)(&extensions, &mut buf)?;
    }
    Ok(buf)
}

pub struct RgbImageWithMetadata<T>(pub Rgb8Image, pub T);

impl<T> RgbImageWithMetadata<T> {
    pub fn new(img: Rgb8Image, meta: T) -> Self {
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
// Currently only supports planar images
fn encode_raw(
    mut buf: Vec<u8>,
    flat_buffer: &[u8],
    pixel_kind: DataType,
    (width, height): (NonZeroU32, NonZeroU32),
    pixel_size: NonZeroU8,
) -> anyhow::Result<Vec<u8>> {
    buf.push(pixel_size.get());
    buf.push(pixel_kind as u8);
    buf.put_slice(&width.get().to_le_bytes());
    buf.put_slice(&height.get().to_le_bytes());
    buf.put_slice(flat_buffer);

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
