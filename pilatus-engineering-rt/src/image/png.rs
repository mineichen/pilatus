use std::{io::Cursor, num::NonZeroU8, sync::Arc};

use image::ImageFormat;
use imbuf::{DynamicImageChannel, Image, ImageChannel, LumaImage};
use minfac::{Registered, ServiceCollection};
use pilatus_engineering::image::{
    DynamicImage, EncodeError, ImageEncoder, ImageEncoderTrait, Rgb8Image,
};

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
        match (first, &input.len(), first.pixel_elements().get()) {
            (DynamicImageChannel::U8(typed), 1, 1) => {
                let img = image::ImageBuffer::<image::Luma<_>, _>::from_raw(
                    width.get(),
                    height.get(),
                    typed.buffer_flat(),
                )
                .expect("u8 Buffer always matches");
                let mut buf = Vec::with_capacity(
                    (width.get() * height.get()) as usize / APPROXIMATE_COMPRESSION,
                );
                img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
                    .map_err(|e| EncodeError::Processing(e.into()))?;
                return Ok(buf.into());
            }
            (DynamicImageChannel::U16(typed), 1, 1) => {
                let img = image::ImageBuffer::<image::Luma<_>, _>::from_raw(
                    width.get(),
                    height.get(),
                    typed.buffer_flat(),
                )
                .expect("u16 Buffer always matches");
                let mut buf = Vec::with_capacity(
                    (width.get() * height.get() * 2) as usize / APPROXIMATE_COMPRESSION,
                );
                img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
                    .map_err(|e| EncodeError::Processing(e.into()))?;
                return Ok(buf.into());
            }
            (DynamicImageChannel::U8(_typed), 3, 1) => {
                let image = Image::<u8, 3>::try_from(input)
                    .map_err(|input| EncodeError::Unknown(format!("{input:?}")))?;
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
            (DynamicImageChannel::U8(typed), 1, 3) => {
                let img = image::ImageBuffer::<image::Rgb<_>, _>::from_raw(
                    width.get(),
                    height.get(),
                    typed.buffer_flat(),
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

    use super::*;

    #[test]
    fn encode_color_image() {
        let raw = vec![0, 0, 0, 0, 128, 128, 128, 128, 255, 255, 255, 255];
        let non_zero_two = NonZeroU32::try_from(2).unwrap();
        let rgb = Image::<u8, 3>::new_vec(raw.clone(), non_zero_two, non_zero_two);
        //crate::image::DynamicImage::Rgb8Planar(rgb.clone());
        let backend = PngBackend {};
        let encoded = backend.encode(DynamicImage::from(rgb.clone())).unwrap();
        let d_image = Rgb8Image::try_from(rgb.clone()).unwrap();
        let decoded = image::load_from_memory(&encoded).unwrap();
        let pixels = decoded.pixels().map(|(_x, _y, i)| i).collect::<Vec<_>>();
        assert_eq!(
            (0..4)
                .map(|_| image::Rgba([0, 128, 255, 255]))
                .collect::<Vec<_>>(),
            pixels
        );
        let rgb_back = Rgb8Image::try_from(d_image).unwrap().into_planar();

        assert_eq!(
            rgb_back,
            rgb,
            "{:?} {:?}",
            rgb_back.buffers(),
            rgb.buffers()
        );
    }

    #[test]
    fn encode_and_decode_planar_rgb_image() {
        let image = image::ImageBuffer::<image::Rgb<u8>, _>::from_vec(
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
        let pilatus =
            Image::<[u8; 3], 1>::new_vec(vec_rgb, 2.try_into().unwrap(), 2.try_into().unwrap());
        let dynamic: DynamicImage = pilatus.clone().into();
        let dynamic2: DynamicImage = pilatus.into();

        let rgb: Rgb8Image = dynamic.try_into().expect("Buffer contains no rgb-image");
        let inter = rgb.into_planar();
        let [r, g, b] = inter.buffers();
        assert_eq!([0, 3, 6, 9], r);
        assert_eq!([1, 4, 7, 10], g);
        assert_eq!([2, 5, 8, 11], b);

        let backend = PngBackend {};
        let png = backend.encode(dynamic2).unwrap();
        let image::DynamicImage::ImageRgb8(reloaded) = image::load_from_memory(&png).unwrap()
        else {
            panic!("Buffer contains rgb-image");
        };
        assert_eq!(reloaded, image);
    }
}
