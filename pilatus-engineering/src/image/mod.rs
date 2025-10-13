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
use std::{
    borrow::Cow,
    fmt::Debug,
    num::{NonZero, NonZeroU32, TryFromIntError},
    sync::Arc,
};

use crate::{InvertibleTransform, InvertibleTransform3d};
pub use image_buffer::GenericImage;

#[cfg(feature = "tokio")]
mod broadcaster;
#[cfg(feature = "image-algorithm")]
mod logo;
mod message;
mod meta;
#[cfg(feature = "image-algorithm")]
mod png;
mod stable_hash;

#[cfg(feature = "tokio")]
pub use broadcaster::*;
use image::GenericImageView;
#[cfg(feature = "image-algorithm")]
pub use logo::*;
pub use meta::*;

pub use message::*;
#[cfg(feature = "image-algorithm")]
pub use png::*;
pub use stable_hash::*;

pub trait PointProjector {
    fn project_to_world_plane(
        &self,
        transform: &InvertibleTransform,
    ) -> Result<InvertibleTransform3d, anyhow::Error>;
}

pub type DynamicPointProjector = Arc<dyn PointProjector + 'static + Send + Sync>;

pub type LumaImage = GenericImage<u8, 1>;

pub trait RgbImage: Debug {
    fn is_packed(&self) -> bool;
    fn size(&self) -> (NonZeroU32, NonZeroU32);
    /// Returns a buffer with layout RGBRGBRGBRGB
    fn into_packed(self: Arc<Self>) -> Arc<dyn InterleavedRgbImage>;
    /// Returns a buffer with layout RRRRGGGGBBBB
    fn into_unpacked(self: Arc<Self>) -> Arc<dyn UnpackedRgbImage>;
}

/// All bytes are stored in a continuous buffer (y:x:channel)
pub trait InterleavedRgbImage: RgbImage {
    fn flat_buffer(&self) -> &[u8];
    //fn into_flat_vec(self: Arc<Self>) -> Vec<u8>;
}

/// Image consists of one image per channel (channel:y:x)
/// Channels mustn't be continuous in memory
pub trait UnpackedRgbImage {
    fn get_channels(&self) -> [&[u8]; 3];
}

pub type PackedGenericImage = GenericImage<[u8; 3], 1>;

impl RgbImage for PackedGenericImage {
    fn is_packed(&self) -> bool {
        true
    }

    fn into_packed(self: Arc<Self>) -> Arc<dyn InterleavedRgbImage> {
        self
    }

    fn into_unpacked(self: Arc<Self>) -> Arc<dyn UnpackedRgbImage> {
        Arc::new(GenericImage::<u8, 3>::from_interleaved(&self))
    }

    fn size(&self) -> (NonZeroU32, NonZeroU32) {
        self.dimensions()
    }
}

impl InterleavedRgbImage for GenericImage<[u8; 3], 1> {
    fn flat_buffer(&self) -> &[u8] {
        self.flat_buffer()
    }
}

pub type UnpackedGenericImage = GenericImage<u8, 3>;

impl From<PackedGenericImage> for DynamicImage {
    fn from(value: PackedGenericImage) -> Self {
        let planar = UnpackedGenericImage::from_interleaved(&value);
        DynamicImage::Rgb8Planar(planar)
    }
}

impl RgbImage for GenericImage<u8, 3> {
    fn is_packed(&self) -> bool {
        false
    }

    fn into_packed(self: Arc<Self>) -> Arc<dyn InterleavedRgbImage> {
        Arc::new(PackedGenericImage::from_planar_image(&self))
    }

    fn into_unpacked(self: Arc<Self>) -> Arc<dyn UnpackedRgbImage> {
        self
    }

    fn size(&self) -> (NonZeroU32, NonZeroU32) {
        self.dimensions()
    }
}

impl UnpackedRgbImage for UnpackedGenericImage {
    fn get_channels(&self) -> [&[u8]; 3] {
        self.buffers()
    }
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum DynamicImage {
    Luma8(LumaImage),
    Luma16(GenericImage<u16, 1>),
    Rgb8Planar(GenericImage<u8, 3>),
}

impl From<LumaImage> for DynamicImage {
    fn from(value: LumaImage) -> Self {
        DynamicImage::Luma8(value)
    }
}

impl From<GenericImage<u16, 1>> for DynamicImage {
    fn from(value: GenericImage<u16, 1>) -> Self {
        DynamicImage::Luma16(value)
    }
}

impl TryFrom<DynamicImage> for GenericImage<u16, 1> {
    type Error = UnsupportedImageError<DynamicImage>;

    fn try_from(value: DynamicImage) -> Result<Self, Self::Error> {
        if let DynamicImage::Luma16(x) = value {
            Ok(x)
        } else {
            let msg = format!("{value:?}");
            Err(UnsupportedImageError(value, msg.into()))
        }
    }
}

impl<'a> TryFrom<&'a DynamicImage> for &'a GenericImage<u16, 1> {
    type Error = UnsupportedImageError<&'a DynamicImage>;

    fn try_from(value: &'a DynamicImage) -> Result<Self, Self::Error> {
        if let DynamicImage::Luma16(x) = value {
            Ok(x)
        } else {
            let msg = format!("{value:?}");
            Err(UnsupportedImageError(value, msg.into()))
        }
    }
}

impl DynamicImage {
    pub fn dimensions(&self) -> (NonZero<u32>, NonZero<u32>) {
        match self {
            DynamicImage::Luma8(x) => x.dimensions(),
            DynamicImage::Luma16(x) => x.dimensions(),
            DynamicImage::Rgb8Planar(x) => x.dimensions(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Unsupported format {1}")]
pub struct UnsupportedImageError<TIn>(TIn, Cow<'static, str>);

#[derive(Debug, thiserror::Error)]
pub enum ImageConversionError {
    #[error("Dimensions mustn't be 0")]
    InvalidSize(#[from] TryFromIntError),

    #[error("{0}")]
    Unsupported(#[from] UnsupportedImageError<image::DynamicImage>),
}

impl TryFrom<image::DynamicImage> for DynamicImage {
    type Error = ImageConversionError;

    fn try_from(value: image::DynamicImage) -> Result<Self, Self::Error> {
        let (width, height) = value.dimensions();
        let (width, height) = (width.try_into()?, height.try_into()?);
        let invalid_format = match value {
            image::DynamicImage::ImageLuma8(x) => {
                return Ok(DynamicImage::Luma8(GenericImage::new_vec(
                    x.into_vec(),
                    width,
                    height,
                )))
            }
            image::DynamicImage::ImageLuma16(x) => {
                return Ok(DynamicImage::Luma16(GenericImage::new_vec(
                    x.into_vec(),
                    width,
                    height,
                )))
            }
            image::DynamicImage::ImageLumaA8(_) => "ImageLumaA8",
            image::DynamicImage::ImageRgb8(x) => {
                let vec_u8 = x.into_vec();
                let unpacked =
                    UnpackedGenericImage::from_flat_interleaved(&vec_u8, (width, height));
                return Ok(DynamicImage::Rgb8Planar(unpacked));
            }
            image::DynamicImage::ImageRgba8(_) => "ImageRgba8",
            image::DynamicImage::ImageLumaA16(_) => "ImageLumaA16",
            image::DynamicImage::ImageRgb16(_) => "ImageRgb16",
            image::DynamicImage::ImageRgba16(_) => "ImageRgba16",
            image::DynamicImage::ImageRgb32F(_) => "ImageRgb32F",
            image::DynamicImage::ImageRgba32F(_) => "ImageRgba32F",
            _ => "Unknown",
        };
        Err(UnsupportedImageError(value, Cow::Borrowed(invalid_format)).into())
    }
}

#[cfg(test)]
mod tests {
    use image::Rgb;

    use super::*;

    #[test]
    fn miri_load_and_save_dynamic_rgb_image() {
        let image = image::ImageBuffer::<Rgb<u8>, _>::from_vec(
            2,
            2,
            vec![0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        )
        .unwrap();
        let vec_u8 = image.clone().into_vec();
        let vec_rgb: Vec<[u8; 3]> = vec_u8
            .chunks_exact(3)
            .map(|chunk| [chunk[0], chunk[1], chunk[2]])
            .collect();
        let pilatus = GenericImage::<[u8; 3], 1>::new_vec(
            vec_rgb,
            2.try_into().unwrap(),
            2.try_into().unwrap(),
        );
        let dynamic: DynamicImage = pilatus.into();

        let DynamicImage::Rgb8Planar(rgb) = &dynamic else {
            panic!("Buffer contains no rgb-image");
        };

        let [r, g, b] = rgb.buffers();
        assert_eq!([0, 3, 6, 9], r);
        assert_eq!([1, 4, 7, 10], g);
        assert_eq!([2, 5, 8, 11], b);
        let png = dynamic.encode_png().unwrap();
        let image::DynamicImage::ImageRgb8(reloaded) = image::load_from_memory(&png).unwrap()
        else {
            panic!("Buffer contains rgb-image");
        };
        assert_eq!(reloaded, image);
    }

    #[test]
    fn miri_test_into_packed() {
        let size = 2.try_into().unwrap();
        let image = Arc::new(GenericImage::<u8, 3>::new_vec(
            vec![1u8, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3],
            size,
            size,
        ));
        let packed = image.into_packed();
        assert_eq!(
            packed.flat_buffer(),
            vec!(1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3)
        );
    }
}
