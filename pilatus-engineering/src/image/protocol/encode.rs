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
use imbuf::{
    DynamicImage, DynamicImageChannel, DynamicSize, Image, ImageChannel, PixelTypePrimitive,
};
use jpeg_encoder::{ColorType, Encoder};
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
pub struct EncodeExtension {
    pub kind: u8,
    pub writer: EncodeExtensionWriter,
}
#[derive(Default)]
pub struct EncodeExtensions(HashMap<u8, EncodeExtensionWriter>);

impl EncodeExtensions {
    pub fn new(extensions: impl IntoIterator<Item = EncodeExtension>) -> Self {
        let iter = extensions.into_iter().map(|x| (x.kind, x.writer));
        Self(into_extensions_map(iter))
    }
}

impl StreamableImage for LumaImage {
    fn encode(self, _encoder: &MetaImageEncoder) -> anyhow::Result<Vec<u8>> {
        let dims = self.dimensions();
        encode_legacy(self.buffer(), ColorType::Luma, dims, |_| Ok(()))
    }
}

#[derive(Default, serde::Deserialize, Clone, Copy)]
pub enum StreamingImageFormat {
    #[default]
    Jpeg,
    Raw,
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
    extensions: Arc<EncodeExtensions>,
}

impl MetaImageEncoder {
    pub fn with_extensions(extensions: Arc<EncodeExtensions>) -> Self {
        Self { extensions }
    }

    pub fn encode(
        &self,
        image: Result<ImageWithMeta<DynamicImage>, StreamImageError<DynamicImage>>,
        format: StreamingImageFormat,
    ) -> anyhow::Result<Vec<u8>> {
        match image {
            Ok(x) => format.encode_dynamic_image(
                CODE_OK,
                x.image,
                x.extensions,
                x.meta,
                &self.extensions,
            ),
            Err(e) => match e {
                #[expect(deprecated)]
                StreamImageError::MissedItems(MissedItemsError { number, .. }) => {
                    encode_meta(prepare_buffer(CODE_MISSED_ITEM, 12), |x| {
                        Ok(x.write_all(&number.0.to_le_bytes())?)
                    })
                }
                StreamImageError::ProcessingError { image, error } => {
                    debug!("Processing error: {error}");
                    format.encode_dynamic_image(
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

pub struct MetaImageEncodeTask {
    pub image: Result<ImageWithMeta<DynamicImage>, StreamImageError<DynamicImage>>,
    pub format: StreamingImageFormat,
}

impl StreamableImage for MetaImageEncodeTask {
    fn encode(self, encoder: &MetaImageEncoder) -> anyhow::Result<Vec<u8>> {
        encoder.encode(self.image, self.format)
    }
}

impl StreamingImageFormat {
    fn encode_dynamic_image<T: Serialize>(
        self,
        code: u16,
        image: DynamicImage,
        ext: AnyMultiMap,
        meta: T,
        write_extensions: &EncodeExtensions,
    ) -> anyhow::Result<Vec<u8>> {
        match self {
            StreamingImageFormat::Jpeg => encode_dynamic_jpeg_image(code, image, meta),
            StreamingImageFormat::Raw => {
                encode_dynamic_raw_image(code, image, ext, meta, write_extensions)
            }
        }
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

fn encode_dynamic_raw_image<T: Serialize>(
    flag: u16,
    image: DynamicImage,
    extensions: AnyMultiMap,
    meta: T,
    write_extensions: &EncodeExtensions,
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

fn encode_dynamic_jpeg_image<T: Serialize>(
    flag: u16,
    image: DynamicImage,
    meta: T,
) -> anyhow::Result<Vec<u8>> {
    let first = image.first();
    let dims = first.dimensions();
    let pixel_elements = first.pixel_elements();

    let buf = prepare_dynamic_image_buf(
        flag,
        meta,
        dims.0.get() as usize * dims.1.get() as usize / 2,
    )?;
    match (first, image.len(), pixel_elements.get()) {
        (DynamicImageChannel::U8(i), 1, 1) => {
            encode_jpeg(buf, i.buffer_flat(), ColorType::Luma, dims)
        }
        // This code was once active, but is wrong... We should just say its not supported
        // (DynamicImageChannel::U16(i), 1, 1) => {
        //     let mut_buf = i
        //         .buffer_flat()
        //         .iter()
        //         .map(|x| (x >> 8) as u8)
        //         .collect::<Vec<_>>();
        //     encode_jpeg(buf, &mut_buf, ColorType::Luma, dims)
        // }
        (DynamicImageChannel::U8(_typed), 3, 1) => {
            let image = Image::<u8, 3>::try_from(image)?;
            let interleaved = Image::<[u8; 3], 1>::from_planar_image(&image);
            encode_jpeg(buf, interleaved.buffer_flat(), ColorType::Rgb, dims)
        }
        _ => Err(anyhow!("Unsupported image format: {:?}", image)),
    }
}

impl<T: Serialize> StreamableImage for (LumaImage, T) {
    fn encode(self, _encoder: &MetaImageEncoder) -> anyhow::Result<Vec<u8>> {
        let dims = self.0.dimensions();
        encode_legacy(self.0.buffer(), ColorType::Luma, dims, |b| {
            Ok(serde_json::to_writer(b, &self.1)?)
        })
    }
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

impl<T: Serialize> StreamableImage for RgbImageWithMetadata<T> {
    fn encode(self, _encoder: &MetaImageEncoder) -> anyhow::Result<Vec<u8>> {
        let dims = self.0.dimensions();
        let packed = self.0.into_interleaved();
        encode_legacy(packed.buffer_flat(), ColorType::Rgb, dims, |b| {
            Ok(serde_json::to_writer(b, &self.1)?)
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
