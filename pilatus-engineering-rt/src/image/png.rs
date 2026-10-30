use std::{io::Cursor, sync::Arc};

use image::ImageFormat;
use imbuf::Image;
use minfac::ServiceCollection;
use pilatus_engineering::image::{DynamicImage, EncodeError, ImageEncoder, ImageEncoderTrait};

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.register(|| Arc::new(PngBackend {}))
        .alias(|s| ImageEncoder(s as _));
}

struct PngBackend {}
impl ImageEncoderTrait for PngBackend {
    fn encode(&self, input: DynamicImage) -> Result<bytes::Bytes, EncodeError> {
        const APPROXIMATE_COMPRESSION: usize = 3;
        let first = input.first();
        let (width, height) = first.dimensions();
        let to_processing_error =
            |e: imbuf::IncompatibleChannelError| EncodeError::Processing(e.into());
        match (
            first.bytes_per_primitive().get(),
            input.len(),
            first.pixel_elements().get(),
        ) {
            (1, 1, 1) => {
                let img = image::ImageBuffer::<image::Luma<_>, _>::from_raw(
                    width.get(),
                    height.get(),
                    first.buffer_flat::<u8>().map_err(to_processing_error)?,
                )
                .expect("u8 Buffer always matches");
                let mut buf = Vec::with_capacity(
                    (width.get() * height.get()) as usize / APPROXIMATE_COMPRESSION,
                );
                img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
                    .map_err(|e| EncodeError::Processing(e.into()))?;
                return Ok(buf.into());
            }
            (2, 1, 1) => {
                let img = image::ImageBuffer::<image::Luma<_>, _>::from_raw(
                    width.get(),
                    height.get(),
                    first.buffer_flat::<u16>().map_err(to_processing_error)?,
                )
                .expect("u16 Buffer always matches");
                let mut buf = Vec::with_capacity(
                    (width.get() * height.get() * 2) as usize / APPROXIMATE_COMPRESSION,
                );
                img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
                    .map_err(|e| EncodeError::Processing(e.into()))?;
                return Ok(buf.into());
            }
            (1, 3, 1) => {
                let image = Image::<u8, 3>::try_from(input)
                    .map_err(|e| EncodeError::Unknown(format!("{e:?}")))?;
                let interleaved = Image::<[u8; 3], 1>::from_planar_image(&image);
                let img = image::ImageBuffer::<image::Rgb<_>, _>::from_raw(
                    width.get(),
                    height.get(),
                    interleaved.buffer_flat(),
                )
                .expect("u8 Color Buffer always matches");
                let mut buf = Vec::with_capacity(
                    (width.get() * height.get() * 3) as usize / APPROXIMATE_COMPRESSION,
                );
                img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
                    .map_err(|e| EncodeError::Processing(e.into()))?;
                return Ok(buf.into());
            }
            (1, 1, 3) => {
                let img = image::ImageBuffer::<image::Rgb<_>, _>::from_raw(
                    width.get(),
                    height.get(),
                    first.buffer_flat::<u8>().map_err(to_processing_error)?,
                )
                .expect("u8 Color Buffer always matches");
                let mut buf = Vec::with_capacity(
                    (width.get() * height.get() * 3) as usize / APPROXIMATE_COMPRESSION,
                );
                img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
                    .map_err(|e| EncodeError::Processing(e.into()))?;
                return Ok(buf.into());
            }
            _ => {}
        }
        Err(EncodeError::Unknown(format!("{input:?}")))
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use image::GenericImageView;
    use pilatus_engineering::image::{DynamicImage, Image, Rgb8Image};
    use testresult::TestResult;

    use super::*;

    #[test]
    fn encode_color_image() -> TestResult {
        let raw = vec![0, 0, 0, 0, 128, 128, 128, 128, 255, 255, 255, 255];
        let non_zero_two = NonZeroU32::try_from(2)?;
        let rgb = Image::<u8, 3>::new_vec(raw.clone(), non_zero_two, non_zero_two)?;
        //crate::image::DynamicImage::Rgb8Planar(rgb.clone());
        let backend = PngBackend {};
        let encoded = backend.encode(DynamicImage::from(rgb.clone()))?;
        let d_image = Rgb8Image::from(rgb.clone());
        let decoded = image::load_from_memory(&encoded)?;
        let pixels = decoded.pixels().map(|(_x, _y, i)| i).collect::<Vec<_>>();
        assert_eq!(
            (0..4)
                .map(|_| image::Rgba([0, 128, 255, 255]))
                .collect::<Vec<_>>(),
            pixels
        );
        let rgb_back = d_image.into_planar();

        assert_eq!(
            rgb_back,
            rgb,
            "{:?} {:?}",
            rgb_back.buffers(),
            rgb.buffers()
        );
        Ok(())
    }

    #[test]
    fn encode_and_decode_planar_rgb_image() -> TestResult {
        let image = image::ImageBuffer::<image::Rgb<u8>, _>::from_vec(
            2,
            2,
            vec![0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        )
        .ok_or("Buffer doesn't match the dimensions")?;
        let vec_u8 = image.clone().into_vec();
        let vec_rgb: Vec<[u8; 3]> = vec_u8
            .chunks_exact(3)
            .map(|chunk| [chunk[0], chunk[1], chunk[2]])
            .collect();
        let pilatus = Image::<[u8; 3], 1>::new_vec(vec_rgb, 2.try_into()?, 2.try_into()?)?;
        let dynamic: DynamicImage = pilatus.clone().into();
        let dynamic2: DynamicImage = pilatus.into();

        let rgb: Rgb8Image = dynamic.try_into()?;
        let inter = rgb.into_planar();
        let [r, g, b] = inter.buffers();
        assert_eq!([0, 3, 6, 9], r);
        assert_eq!([1, 4, 7, 10], g);
        assert_eq!([2, 5, 8, 11], b);

        let backend = PngBackend {};
        let png = backend.encode(dynamic2)?;
        let image::DynamicImage::ImageRgb8(reloaded) = image::load_from_memory(&png)? else {
            panic!("Buffer contains rgb-image");
        };
        assert_eq!(reloaded, image);
        Ok(())
    }
}
