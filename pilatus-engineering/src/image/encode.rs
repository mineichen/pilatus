/// # Reason for introducing new protocol
/// The initial protocoll stopped if a error occured. Such errors could be temporal (camera not found) or fixable by changing parameters back)
/// In that case, the subscriber started to randomly request new frames without knowing if this is reasonable
/// The new design still allows all previous workflows by simply adding .take_while() and therefore volunatarely close the stream.
/// Furthermore, the new design allows errors to contain images, for situations, where e.g.
use std::{
    fmt::{self, Debug, Formatter},
    num::{NonZeroU16, NonZeroU32, NonZeroU8},
};

use anyhow::anyhow;
use bytes::BufMut;
use imbuf::{DynamicImageChannel, DynamicSize, ImageChannel, PixelTypePrimitive};
use jpeg_encoder::{ColorType, Encoder};
use serde::Serialize;
use tracing::{debug, trace};

use super::{DynamicImage, Image, ImageWithMeta, LumaImage, Rgb8Image, StreamImageError};

pub trait StreamableImage: Sized {
    fn encode(self) -> anyhow::Result<Vec<u8>>;
}

impl StreamableImage for LumaImage {
    fn encode(self) -> anyhow::Result<Vec<u8>> {
        let dims = self.dimensions();
        encode_legacy(self.buffer(), ColorType::Luma, dims, |_| Ok(()))
    }
}

const OK_CODE: u8 = 0 << 4;
const MISSED_ITEM_CODE: u8 = 1 << 4;
const PROCESSING_CODE: u8 = 2 << 4;
const ACTOR_ERROR_CODE: u8 = 3 << 4;
#[allow(dead_code)]
const ACQUISITION_CODE: u8 = 4 << 4;

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
            Ok(x) => {
                let meta = x.meta;
                self.1.encode_dynamic_image(OK_CODE, x.image, meta)
            }
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
    fn encode_typed<TMeta: Serialize, TPixel: PixelTypePrimitive>(
        x: &ImageChannel<DynamicSize<TPixel>>,
        pixel_kind: DataType,
        channel_len: NonZeroU16,
        flag: u8,
        meta: TMeta,
    ) -> anyhow::Result<Vec<u8>> {
        let (width, height) = x.dimensions();
        let pixel_elements = x.pixel_elements();
        //let pixel_len = x.width().get() * x.height().get();
        let buf = prepare_dynamic_image_buf(
            flag,
            meta,
            width.get() as usize * height.get() as usize / 2 * pixel_elements.get() as usize,
        )?;
        encode_raw(
            buf,
            x.buffer_flat_bytes(),
            pixel_kind,
            (width, height),
            pixel_elements,
            channel_len,
        )
    }
    let channels =
        NonZeroU16::new(u16::try_from(image.len())?).expect("Images cannot have 0 channels");
    match image.first() {
        imbuf::DynamicImageChannel::U8(x) => encode_typed(x, DataType::U8, channels, flag, meta),
        imbuf::DynamicImageChannel::U16(x) => encode_typed(x, DataType::U16, channels, flag, meta),
        _ => Err(anyhow!("Unsupported image format: {:?}", image)),
    }
}

fn encode_dynamic_jpeg_image<T: Serialize>(
    flag: u8,
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
            let image = Image::<u8, 3>::try_from(image).map_err(|e| anyhow!("{e:?}"))?;
            let interleaved = Image::<[u8; 3], 1>::from_planar_image(&image);
            encode_jpeg(buf, interleaved.buffer_flat(), ColorType::Rgb, dims)
        }
        _ => Err(anyhow!("Unsupported image format: {:?}", image)),
    }
}

impl<T: Serialize> StreamableImage for (LumaImage, T) {
    fn encode(self) -> anyhow::Result<Vec<u8>> {
        let dims = self.0.dimensions();
        encode_legacy(self.0.buffer(), ColorType::Luma, dims, |b| {
            serde_json::to_writer(b, &self.1).map_err(Into::into)
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
    fn encode(self) -> anyhow::Result<Vec<u8>> {
        let dims = self.0.dimensions();
        let packed = self.0.into_interleaved();
        encode_legacy(packed.buffer_flat(), ColorType::Rgb, dims, |b| {
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

// Currently only supports planar images
fn encode_raw(
    mut buf: Vec<u8>,
    flat_buffer: &[u8],
    pixel_kind: DataType,
    (width, height): (NonZeroU32, NonZeroU32),
    pixel_size: NonZeroU8,
    channel_size: NonZeroU16,
) -> anyhow::Result<Vec<u8>> {
    // https://stackoverflow.com/questions/45213511/formula-for-memory-alignment
    let unaligned_pixel_start = buf.len() + 4;
    let alignment_bytes = (((unaligned_pixel_start + 7) & !7) - unaligned_pixel_start) as u32;

    const HEADER_BYTE_SIZE: u32 = 8;
    let image_total_buf_len: u32 = flat_buffer.len().try_into()?;
    buf.extend_from_slice(
        &(image_total_buf_len + HEADER_BYTE_SIZE + alignment_bytes).to_le_bytes(),
    );

    buf.extend((0..alignment_bytes).map(|_| 0)); // Guarantee 8Byte aligned
    buf.push(pixel_size.get()); // reserved
    buf.push(pixel_kind as u8);
    buf.put_slice(&channel_size.get().to_le_bytes());
    buf.put_slice(&width.get().to_le_bytes());
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
