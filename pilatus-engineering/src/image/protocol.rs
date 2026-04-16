mod decode;
#[cfg(feature = "encode")]
mod encode;

use std::num::{NonZeroU32, NonZeroU8};

use anyhow::Context;
pub use decode::*;
#[cfg(feature = "encode")]
pub use encode::*;

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
const KIND_MASK: u8 = 2 << 4;

pub trait StreamableImage: Sized {
    fn encode(self) -> anyhow::Result<Vec<u8>>;
}

#[repr(u8)]
pub(crate) enum DataType {
    U8,
    U16,
}

const MASK_SENTINEL: [u8; 15] = {
    let mut sentinel = [0; 15];
    sentinel[14] = 42;
    sentinel
};

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

#[cfg(all(test, feature = "encode"))]
mod tests {
    use std::num::NonZeroU32;

    use imask::ImaskSet;
    use imbuf::{Image, PixelType};
    use testresult::TestResult;

    use crate::image::{ImageWithMeta, StableHash};

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

        let bytes = (Ok(encodable_image.clone()), StreamingImageFormat::Raw).encode()?;
        let back = crate::image::decode(&bytes)??.try_convert_image::<Image<T, CHANNELS>>()?;
        assert_eq!(image.buffers(), back.image.buffers());
        assert_eq!(encodable_image.meta, back.meta);

        Ok(())
    }
    #[test]
    fn encode_decode_with_imask() -> TestResult {
        type Ranges = imask::SortedRanges<u64, u64>;
        let image = Image::<u8, 1>::new_vec_flat(vec![128], NonZeroU32::MIN, NonZeroU32::MIN);
        let width = const { NonZeroU32::new(10).unwrap() };
        let height = const { NonZeroU32::new(20).unwrap() };
        let roi = imask::Rect::new(100, 100, width, height);
        let ranges = [
            Ranges::try_from_ordered_iter([0u32..10, 15..20].with_bounds(width, height))?,
            Ranges::try_from_ordered_iter([30u32..40, 45..50].with_roi(roi))?,
        ];
        let mut meta = ImageWithMeta::with_hash(image.clone().into(), None);
        for range in ranges.iter() {
            meta.extensions.insert(range.clone());
        }
        assert_eq!(
            ranges.clone().to_vec(),
            meta.extensions
                .iter::<Ranges>()
                .map(|x| x.clone())
                .collect::<Vec<_>>()
        );

        let encodable_image = Ok(meta);

        let bytes = (encodable_image, StreamingImageFormat::Raw).encode()?;
        let back = crate::image::decode(&bytes)??.try_convert_image::<Image<u8, 1>>()?;

        assert_eq!(image.buffers(), back.image.buffers());
        assert_eq!(
            ranges.clone().to_vec(),
            back.extensions
                .iter::<Ranges>()
                .map(|x| x.clone())
                .collect::<Vec<_>>()
        );

        Ok(())
    }
}
