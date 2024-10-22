use std::io::Cursor;

use image::ImageFormat;

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
        match self {
            Self::Luma8(i) => {
                let (width, height) = i.dimensions();
                let img = image::ImageBuffer::<image::Luma<_>, _>::from_raw(
                    width.get(),
                    height.get(),
                    i.buffer(),
                )
                .expect("u8 Buffer always matches");
                let mut buf = Vec::with_capacity((width.get() * height.get()) as usize / 3);
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
                let mut buf = Vec::with_capacity((width.get() * height.get() * 2) as usize / 3);
                img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;
                Ok(buf)
            } //i => Err(EncodeError::Unknown(format!("{i:?}"))),
        }
    }
}
