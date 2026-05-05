use std::{
    collections::HashMap,
    io,
    num::{NonZeroU16, NonZeroU32, NonZeroU8, Saturating},
    ops::{Deref, DerefMut},
    sync::Arc,
};

use anyhow::{anyhow, Context};
use imbuf::{DynamicImage, DynamicImageChannel, ImageChannel};
use pilatus::MissedItemsError;
use serde::de::DeserializeOwned;
use tracing::warn;

use crate::image::{
    protocol::{calculate_buf_len, into_extensions_map},
    AnyMultiMap, DataType, ImageWithMeta, StreamImageError,
};
type DecodeExtensionReader =
    Box<dyn for<'a> Fn(&'a [u8], &mut AnyMultiMap) -> io::Result<&'a [u8]> + Send + Sync>;
pub struct MetaDecodeExtension {
    pub kind: u8,
    pub reader: DecodeExtensionReader,
}

#[derive(Default)]
pub struct MetaDecodeExtensions(HashMap<u8, DecodeExtensionReader>);

impl MetaDecodeExtensions {
    pub fn new(extensions: impl IntoIterator<Item = MetaDecodeExtension>) -> Self {
        let iter = extensions.into_iter().map(|x| (x.kind, x.reader));
        Self(into_extensions_map(iter))
    }
}

pub struct AlignedBuf(Vec<u64>);

impl Deref for AlignedBuf {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        unsafe { std::slice::from_raw_parts(self.0.as_ptr() as *const u8, self.0.len() * 8) }
    }
}

impl DerefMut for AlignedBuf {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { std::slice::from_raw_parts_mut(self.0.as_mut_ptr() as *mut u8, self.0.len() * 8) }
    }
}

impl From<AlignedBuf> for Vec<u8> {
    fn from(mut value: AlignedBuf) -> Self {
        let r = unsafe {
            Vec::from_raw_parts(
                value.0.as_mut_ptr() as *mut u8,
                value.0.len() * 8,
                value.0.capacity() * 8,
            )
        };
        std::mem::forget(value.0);
        r
    }
}

/// Returns `Ok(None)` for MissingFrame error
// imbuf::Image<[u8; 3], 1>
#[derive(Clone)]
pub struct MetaImageDecoder {
    extensions: Arc<MetaDecodeExtensions>,
}

impl MetaImageDecoder {
    pub fn with_extensions(extensions: Arc<MetaDecodeExtensions>) -> Self {
        Self { extensions }
    }

    /// Returns a nested Error... The outer is due to encoding/decoding/io, the inner is expected to happen
    pub fn decode(
        &self,
        input: &[u8],
    ) -> anyhow::Result<Result<ImageWithMeta<DynamicImage>, StreamImageError<DynamicImage>>> {
        if input.len() < 8 {
            return Err(anyhow!(
                "Header is only {} bytes long: {:?}",
                input.len(),
                input
            ));
        }
        debug_assert_eq!(input[2], 0, "Using reserved space input[2]: {}", input[2]);
        debug_assert_eq!(input[3], 0, "Using reserved space input[3]: {}", input[3]);
        match u16::from_le_bytes([input[0], input[1]]) {
            super::CODE_OK => extract_metaimage(input, &self.extensions),
            #[expect(deprecated)]
            super::CODE_MISSED_ITEM => {
                let number = input
                    .get(5..)
                    .and_then(|s| serde_json::from_slice(s).ok())
                    .unwrap_or_else(|| {
                        warn!("Couldn't read missed_items {input:?}, use u16::MAX instead");
                        u16::MAX
                    });
                let number = Saturating(number);
                Ok(Err(StreamImageError::MissedItems(MissedItemsError::new(
                    number,
                ))))
                // return Err(anyhow!(
                //     "Get MISSING_FRAME_ERROR({number:?}), which is no longer supported and should be migrated to MetaData {{ missing_frames }} in all InputItems",
                // ));
            }
            super::CODE_PROCESSING => {
                println!("ExtractMetaImage");

                let (msg, image, _rest) = extract_meta_and_image::<String>(input)?;
                Ok(Err(StreamImageError::ProcessingError {
                    image,
                    error: Arc::new(anyhow!("{msg}")),
                }))
            }
            _ => {
                let version = input[1];
                let command_nr = (input[0] & 0b11110000) >> 4;
                Err(if version != super::VERSION {
                    let input_start = &input[0..input.len().min(20)];
                    anyhow!(
                    "Unexpected version for command {command_nr}: Decoder: {}, Encoder: {version}, got: {:?} ({})",
                    super::VERSION,
                    input_start,
                    String::from_utf8_lossy(input_start)
                )
                } else {
                    anyhow!("Stream item with error, which is not yet supported: {command_nr}")
                })
            }
        }
    }
}

fn extract_meta_and_image<T: DeserializeOwned>(
    input: &[u8],
) -> anyhow::Result<(T, DynamicImage, &[u8])> {
    let meta_content_len = u32::from_le_bytes(array(&input[4..8]));
    let meta_bytes = input.get(8..meta_content_len as usize + 8).ok_or_else(|| {
        anyhow!(
            "Metadata out of bounds. expected: {}, remaining-input: {}",
            meta_content_len,
            input.len(),
        )
    })?;
    let meta: T = serde_json::from_slice(meta_bytes)?;
    let after_meta: usize = 4 + 4 + meta_content_len as usize;
    if input.len() < after_meta + 8 {
        return Err(anyhow!(
            "Before image is not long enouth:{}, {:?}",
            input.len(),
            &input[after_meta..]
        ));
    }

    let (image, input) = read_image(&input[after_meta..])?;
    Ok((meta, image, input))
}

fn extract_metaimage(
    input: &[u8],
    extensions: &MetaDecodeExtensions,
) -> anyhow::Result<Result<ImageWithMeta<DynamicImage>, StreamImageError<DynamicImage>>> {
    let (meta, image, mut input) = extract_meta_and_image(input)?;
    let mut meta_image = ImageWithMeta::with_meta(image, meta);

    loop {
        match input {
            [super::KIND_IMAGE, rest @ ..] => {
                let (_image, rest) = read_image(rest)?;
                input = rest;
            }
            [kind, rest @ ..] => {
                let ext = extensions.0.get(kind).ok_or_else(|| {
                    anyhow!("Unknown extension {kind}. Add the corresponding encoder. In the future, a per request kind-whitelist should be added to only get known extensions for e.g. HTTP")
                })?;
                input = (ext)(rest, &mut meta_image.extensions)?;
            }
            _ => break,
        }
    }
    Ok(Ok(meta_image))
}

fn read_image(input: &[u8]) -> anyhow::Result<(DynamicImage, &[u8])> {
    let channel_size = NonZeroU16::new(u16::from_le_bytes(array(&input[0..2])))
        .context("channel_size must be > 0")?;
    let mut input = &input[2..];
    let mut err = anyhow::Ok(());
    let mut images_iter = (0..channel_size.get())
        .map(|_| match extract_channel(input) {
            Ok((ch, rest)) => {
                input = rest;
                Some(ch)
            }
            Err(e) => {
                err = Err(e);
                None
            }
        })
        .take_while(|x| x.is_some())
        .map(|x| x.unwrap());
    let first = images_iter
        .next()
        .ok_or_else(|| anyhow!("Expected long enough buffer for first frame"))?;
    let image = DynamicImage::from_channels(first, images_iter);
    err?;
    Ok((image, input))
}

fn extract_channel(input: &[u8]) -> anyhow::Result<(DynamicImageChannel, &[u8])> {
    let (kind, pixel_elements, width, height) = read_raw(input)?;
    let channel_flat_len = calculate_buf_len((width, height), pixel_elements)?;

    let pixel_start = usize::from(super::CHANNEL_HEADER_BYTE_SIZE);
    let pixels = &input[pixel_start..pixel_start + channel_flat_len];

    Ok(match kind {
        DataType::U8 => (
            ImageChannel::new_vec_dynamic(pixels.into(), width, height, pixel_elements).into(),
            &input[pixel_start + channel_flat_len..],
        ),
        DataType::U16 => todo!(),
    })
}

type RawHeader = (DataType, NonZeroU8, NonZeroU32, NonZeroU32);

fn array<T: Copy, const N: usize>(slice: &[T]) -> [T; N] {
    slice.try_into().expect("incorrect_length")
}
fn read_raw(
    input: &[u8],
    //align_bytes: u32,
) -> anyhow::Result<RawHeader> {
    let pixel_size = NonZeroU8::new(input[0]).ok_or_else(|| anyhow!("pixel_size must be > 0... The Backend seems to be newer than the frontend (this was previously reserved space)"))?;
    let kind = DataType::try_from(input[1])?;

    let width: NonZeroU32 = u32::from_le_bytes(array(&input[2..6]))
        .try_into()
        .context("width")?;
    let height: NonZeroU32 = u32::from_le_bytes(array(&input[6..10]))
        .try_into()
        .context("width")?;
    Ok((kind, pixel_size, width, height))
}
