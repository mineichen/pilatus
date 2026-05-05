mod decode;
#[cfg(feature = "encode")]
mod encode;

use std::{
    collections::HashMap,
    num::{NonZeroU32, NonZeroU8},
};

use anyhow::Context;
pub use decode::*;
#[cfg(feature = "encode")]
pub use encode::*;
use tracing::error;

const VERSION: u8 = 1;
const CHANNEL_HEADER_BYTE_SIZE: u8 = 10;

const CODE_OK: u16 = VERSION as u16;
const CODE_MISSED_ITEM: u16 = 1 << 4 | (VERSION as u16) << 8;
#[allow(dead_code)]
const CODE_PROCESSING: u16 = 2 << 4 | (VERSION as u16) << 8;
#[allow(dead_code)]
const CODE_ACTOR_ERROR: u16 = 3 << 4 | (VERSION as u16) << 8;
#[allow(dead_code)]
const CODE_ACQUISITION: u16 = 4 << 4 | (VERSION as u16) << 8;

const KIND_IMAGE: u8 = 1 << 4;

#[cfg(feature = "encode")]
pub trait StreamableImage: Sized {
    fn encode(self, encoder: &MetaImageEncoder) -> anyhow::Result<Vec<u8>>;
}

#[repr(u8)]
pub(crate) enum DataType {
    U8,
    U16,
}

impl TryFrom<u8> for DataType {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::U8),
            1 => Ok(Self::U16),
            _ => Err(anyhow::anyhow!("Unknown DataType {value}")),
        }
    }
}

fn calculate_buf_len(
    (width, height): (NonZeroU32, NonZeroU32),
    pixel_elements: NonZeroU8,
) -> anyhow::Result<usize> {
    let width = usize::try_from(width.get()).context("width")?;
    let height = usize::try_from(height.get()).context("height")?;
    let pixel_elements = usize::from(pixel_elements.get());
    Ok(width * height * pixel_elements)
}

pub(crate) fn into_extensions_map<T>(items: impl IntoIterator<Item = (u8, T)>) -> HashMap<u8, T> {
    let mut conflicts = Vec::new();
    let mut result = HashMap::new();
    for (k, v) in items {
        if let Some(_) = result.insert(k, v) {
            conflicts.push(k);
        }
    }
    if !conflicts.is_empty() {
        error!(
            kind_ids = ?conflicts,
            "Duplicate extension kinds detected. All conflicting extensions are filtered out"
        );
        for conflict in conflicts {
            result.remove(&conflict).unwrap();
        }
    }
    result
}

#[cfg(all(test, feature = "encode"))]
mod tests {
    use std::{num::NonZeroU32, sync::Arc};

    use imbuf::{Image, PixelType};
    use testresult::TestResult;

    use crate::image::{protocol::encode::MetaImageEncoder, ImageWithMeta, StableHash};

    use super::*;

    #[test]
    fn encode_decode_raw_planar() -> TestResult {
        encode_decode_raw::<u8, 3>(vec![0, 32, 64, 128, 192, 255])
    }
    #[test]
    fn encode_decode_raw_interleaved() -> TestResult {
        encode_decode_raw::<[u8; 3], 1>(vec![0, 32, 64, 128, 192, 255])
    }
    #[test]
    fn encode_decode_raw_gray() -> TestResult {
        encode_decode_raw::<u8, 1>(vec![64, 128])
    }

    fn encode_decode_raw<
        T: PixelType + Send + Sync + core::fmt::Debug + Eq,
        const CHANNELS: usize,
    >(
        input: Vec<T::Primitive>,
    ) -> TestResult {
        let image = Image::<T, CHANNELS>::new_vec_flat(
            input,
            const { NonZeroU32::new(1).unwrap() },
            const { NonZeroU32::new(2).unwrap() },
        );
        let hash = StableHash::from_hashable(42);
        let encodable_image = ImageWithMeta::with_hash(image.clone().into(), Some(hash));

        let encoder = MetaImageEncoder::with_extensions(Arc::default());
        let bytes = encoder.encode(Ok(encodable_image.clone()), StreamingImageFormat::Raw)?;
        let decoder = MetaImageDecoder::with_extensions(Arc::default());
        let back = decoder
            .decode(&bytes)??
            .try_convert_image::<Image<T, CHANNELS>>()?;
        assert_eq!(image.buffers(), back.image.buffers());
        assert_eq!(encodable_image.meta, back.meta);

        Ok(())
    }
}
