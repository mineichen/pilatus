//! #Coordinate System
//! There are different interpretations of x and y in imageprocessing. Most work with the top left corner as (0,0)
//! - Web: x=col, y=row https://developer.mozilla.org/en-US/docs/Web/API/CanvasRenderingContext2D/translate
//! - Halcon-Image: x=row + 0.5, y=col + 0.5
//! - Matlab: x=col + 0.5, y=row + 0.5 https://ch.mathworks.com/help/images/image-coordinate-systems.html
//!
//! There are just a few exceptions starting in the lower left corner:
//! - OpenGL: x=col, y=row
//!
//! Pilatus is choosing top left corner (0,0) with x=col, y=row semantics for the following reasons:
//! - All processing-libraries use the top-left corner
//! - Z Points away from the plane. Dept images would have negative pixelvalues otherwise.
//! - Horizontal is commonly described by X and vertical by Y, which leads to less confusion
//! - We need two different formats (web and halcon) anyway. Conversions cannot be avoided
//! - x=row y=col is more widely used in the analyzed examples
//!
//! # Genericity
//! Gray images can easily be shared as Arc<GenericImage<1>>, as there is no confusion how the pixels are aligned
//! Color images are shared via Arc<dyn RgbImage>,
use std::{borrow::Cow, fmt::Debug, num::NonZeroU32, sync::Arc};

use crate::{InvertibleTransform, InvertibleTransform3d};
use imbuf::IncompatibleImageError;
pub use imbuf::{DynamicImage, Image};

#[cfg(feature = "tokio")]
mod broadcaster;
mod logo;
mod message;
mod meta;
mod stable_hash;

#[cfg(feature = "tokio")]
pub use broadcaster::*;
// use image::{GenericImageView, Rgb8Image};
pub use logo::*;
pub use meta::*;

pub use message::*;
pub use stable_hash::*;

pub trait PointProjector {
    fn project_to_world_plane(
        &self,
        transform: &InvertibleTransform,
    ) -> Result<InvertibleTransform3d, anyhow::Error>;
}

pub type DynamicPointProjector = Arc<dyn PointProjector + 'static + Send + Sync>;

pub type LumaImage = Image<u8, 1>;

#[derive(Debug, Clone)]
pub enum Rgb8Image {
    Interleaved(Image<[u8; 3], 1>),
    Planar(Image<u8, 3>),
}

impl Rgb8Image {
    pub fn into_planar(self) -> Image<u8, 3> {
        match self {
            Rgb8Image::Interleaved(image) => {
                let dims = image.dimensions();
                Image::<u8, 3>::from_flat_interleaved(image.buffer_flat(), dims)
            }
            Rgb8Image::Planar(image) => image,
        }
    }

    pub fn into_interleaved(self) -> Image<[u8; 3], 1> {
        match self {
            Rgb8Image::Interleaved(image) => image,
            Rgb8Image::Planar(image) => {
                let (width, height) = image.dimensions();
                Image::<[u8; 3], 1>::from_planar(image.buffers(), width, height)
            }
        }
    }

    pub fn dimensions(&self) -> (NonZeroU32, NonZeroU32) {
        match self {
            Rgb8Image::Interleaved(image) => image.dimensions(),
            Rgb8Image::Planar(image) => image.dimensions(),
        }
    }
}

impl From<Image<u8, 3>> for Rgb8Image {
    fn from(value: Image<u8, 3>) -> Self {
        Self::Planar(value)
    }
}

impl From<Image<[u8; 3], 1>> for Rgb8Image {
    fn from(value: Image<[u8; 3], 1>) -> Self {
        Self::Interleaved(value)
    }
}

impl From<Rgb8Image> for DynamicImage {
    fn from(value: Rgb8Image) -> Self {
        match value {
            Rgb8Image::Interleaved(image) => image.into(),
            Rgb8Image::Planar(image) => image.into(),
        }
    }
}

impl TryFrom<DynamicImage> for Rgb8Image {
    type Error = IncompatibleImageError;

    fn try_from(value: DynamicImage) -> Result<Self, Self::Error> {
        Image::<[u8; 3], 1>::try_from(value)
            .map(Rgb8Image::Interleaved)
            .or_else(|d| {
                Image::<u8, 3>::try_from(d.image)
                    .map(Into::into)
                    .map(Rgb8Image::Planar)
            })
    }
}

pub trait ImageEncoderTrait {
    fn encode(&self, image: DynamicImage) -> Result<bytes::Bytes, EncodeError>;
}

impl ImageEncoderTrait for ImageEncoder {
    fn encode(&self, image: DynamicImage) -> Result<bytes::Bytes, EncodeError> {
        self.0.encode(image)
    }
}

#[derive(Clone)]
pub struct ImageEncoder(pub Arc<dyn ImageEncoderTrait + 'static + Send + Sync>);

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EncodeError {
    #[error("ProcessingError: {0}")]
    Processing(anyhow::Error),
    #[error("Unknown dynamic image: {0}")]
    Unknown(String),
}

#[derive(Debug, thiserror::Error)]
#[error("Unsupported format {1}")]
pub struct UnsupportedImageError<TIn>(TIn, Cow<'static, str>);

#[cfg(feature = "image")]
pub trait FromImage: Sized {
    fn from_image(value: image::DynamicImage) -> Result<Self, std::num::TryFromIntError>;
}

#[cfg(feature = "image")]
impl FromImage for DynamicImage {
    fn from_image(value: image::DynamicImage) -> Result<Self, std::num::TryFromIntError> {
        use image::GenericImageView;

        let (width, height) = value.dimensions();
        let (width, height) = (width.try_into()?, height.try_into()?);
        Ok(match value {
            image::DynamicImage::ImageLuma8(x) => {
                Image::<u8, 1>::new_vec(x.into_vec(), width, height).into()
            }
            image::DynamicImage::ImageLuma16(x) => {
                Image::<u16, 1>::new_vec(x.into_vec(), width, height).into()
            }
            image::DynamicImage::ImageLumaA8(x) => {
                Image::<[u8; 2], 1>::new_vec_flat(x.into_vec(), width, height).into()
            }
            image::DynamicImage::ImageLumaA16(x) => {
                Image::<[u16; 2], 1>::new_vec_flat(x.into_vec(), width, height).into()
            }
            image::DynamicImage::ImageRgb8(x) => {
                Image::<[u8; 3], 1>::new_vec_flat(x.into_vec(), width, height).into()
            }
            image::DynamicImage::ImageRgb16(x) => {
                Image::<[u16; 3], 1>::new_vec_flat(x.into_vec(), width, height).into()
            }
            image::DynamicImage::ImageRgb32F(x) => {
                Image::<[f32; 3], 1>::new_vec_flat(x.into_vec(), width, height).into()
            }
            image::DynamicImage::ImageRgba8(x) => {
                Image::<[u8; 4], 1>::new_vec_flat(x.into_vec(), width, height).into()
            }
            image::DynamicImage::ImageRgba16(x) => {
                Image::<[u16; 4], 1>::new_vec_flat(x.into_vec(), width, height).into()
            }
            image::DynamicImage::ImageRgba32F(x) => {
                Image::<[f32; 4], 1>::new_vec_flat(x.into_vec(), width, height).into()
            }
            _ => {
                tracing::error!("Unexhaustive Enum was extended... returning wrong error");
                return Err(NonZeroU32::try_from(0).unwrap_err());
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_into_packed() {
        let size = 2.try_into().unwrap();
        let image = Image::<u8, 3>::new_vec(vec![1u8, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3], size, size);
        let packed = Rgb8Image::Planar(image).into_interleaved();
        assert_eq!(
            packed.buffer_flat(),
            vec!(1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3)
        );
    }
}
