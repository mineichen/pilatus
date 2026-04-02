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

const CODE_OK: u8 = VERSION;
const CODE_MISSED_ITEM: u8 = 1 << 4 | VERSION;
#[allow(dead_code)]
const CODE_PROCESSING: u8 = 2 << 4 | VERSION;
#[allow(dead_code)]
const CODE_ACTOR_ERROR: u8 = 3 << 4 | VERSION;
#[allow(dead_code)]
const CODE_ACQUISITION: u8 = 4 << 4 | VERSION;

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

    use imbuf::{Image, PixelType};
    use testresult::TestResult;

    use crate::image::ImageWithMeta;

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
        let encodable_image = Ok(ImageWithMeta::with_hash(image.clone().into(), None));

        let bytes = (encodable_image, StreamingImageFormat::Raw).encode()?;
        let back = crate::image::decode(&bytes)??;
        let back_typed = Image::<T, CHANNELS>::try_from(back.image)?;
        assert_eq!(image.buffers(), back_typed.buffers());

        Ok(())
    }
}
