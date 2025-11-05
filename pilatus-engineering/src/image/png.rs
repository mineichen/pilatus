use std::io::Cursor;

use image::ImageFormat;

use super::PackedGenericImage;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EncodeError {
    #[error("ProcessingError: {0}")]
    Processing(#[from] image::ImageError),
    #[error("Unknown dynamic image: {0}")]
    Unknown(String),
}

impl crate::image::DynamicImage {
    pub fn encode_png(&self) -> Result<Vec<u8>, EncodeError> {
        const APPROXIMATE_COMPRESSION: usize = 3;
        match self {
            Self::Luma8(i) => {
                let (width, height) = i.dimensions();
                let img = image::ImageBuffer::<image::Luma<_>, _>::from_raw(
                    width.get(),
                    height.get(),
                    i.buffer(),
                )
                .expect("u8 Buffer always matches");
                let mut buf = Vec::with_capacity(
                    (width.get() * height.get()) as usize / APPROXIMATE_COMPRESSION,
                );
                img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;
                Ok(buf)
            }
            Self::Luma16(i) => {
                let (width, height) = i.dimensions();
                let img = image::ImageBuffer::<image::Luma<_>, _>::from_raw(
                    width.get(),
                    height.get(),
                    i.buffer(),
                )
                .expect("u16 Buffer always matches");
                let mut buf = Vec::with_capacity(
                    (width.get() * height.get() * 2) as usize / APPROXIMATE_COMPRESSION,
                );
                img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;
                Ok(buf)
            }
            Self::Rgb8Planar(i) => {
                let (width, height) = i.dimensions();
                let packed: PackedGenericImage = PackedGenericImage::from_planar_image(i);
                let img = image::ImageBuffer::<image::Rgb<_>, _>::from_raw(
                    width.get(),
                    height.get(),
                    crate::image::InterleavedRgbImage::flat_buffer(&packed),
                )
                .expect("u8 Color Buffer always matches");
                let mut buf = Vec::with_capacity(
                    (width.get() * height.get() * 3) as usize / APPROXIMATE_COMPRESSION,
                );
                img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;
                Ok(buf)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use image::GenericImageView;

    use crate::image::GenericImage;

    #[test]
    fn encode_color_image() {
        let raw = vec![0, 0, 0, 0, 128, 128, 128, 128, 255, 255, 255, 255];
        let non_zero_two = NonZeroU32::try_from(2).unwrap();
        let rgb = GenericImage::<u8, 3>::new_vec(raw.clone(), non_zero_two, non_zero_two);
        let d_image = crate::image::DynamicImage::Rgb8Planar(rgb.clone());
        let encoded = d_image.encode_png().unwrap();
        let decoded = image::load_from_memory(&encoded).unwrap();
        let pixels = decoded.pixels().map(|(_x, _y, i)| i).collect::<Vec<_>>();
        assert_eq!(
            (0..4)
                .map(|_| image::Rgba([0, 128, 255, 255]))
                .collect::<Vec<_>>(),
            pixels
        );
        let crate::image::DynamicImage::Rgb8Planar(dynamic) = decoded.try_into().unwrap() else {
            panic!("Should be a RGB image");
        };
        assert_eq!(dynamic, rgb, "{:?} {:?}", dynamic.buffers(), rgb.buffers());
    }
}
