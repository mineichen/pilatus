use anyhow::{Context, anyhow};
use pilatus::RelativeResourcePath;

/// Collection images are stored as `{collection}/{image}.png` (or deeper for recordings).
/// Files at the device root are not accessible through the standard file api.
pub(super) fn is_responsible(path: &RelativeResourcePath) -> bool {
    let path = path.get_path();
    path.components().count() > 1
        && path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("png"))
}

/// Decodes the uploaded image and re-encodes it as PNG.
///
/// Re-encoding is also a safety feature: only plain pixel data is stored,
/// potentially dangerous chunks/tags and metadata (e.g. EXIF) are stripped.
/// Float images (e.g. float TIFFs) are not supported at this point.
pub(super) fn normalize_to_png(data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
    let img =
        image::load_from_memory(data).map_err(|e| anyhow!("File is not a valid image: {e}"))?;

    if matches!(
        img,
        image::DynamicImage::ImageRgb32F(_) | image::DynamicImage::ImageRgba32F(_)
    ) {
        return Err(anyhow!("Float images (e.g. float TIFFs) are not supported"));
    }

    let mut png_data = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_data),
        image::ImageFormat::Png,
    )
    .context("Failed to re-encode image as PNG")?;
    Ok(png_data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    fn png_bytes() -> Vec<u8> {
        let img = ImageBuffer::from_pixel(2, 2, Rgb([255u8, 0, 0]));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    fn jpg_bytes() -> Vec<u8> {
        let img = ImageBuffer::from_pixel(2, 2, Rgb([0u8, 255, 0]));
        let mut buf = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
        buf
    }

    fn tiff_bytes() -> Vec<u8> {
        let img = ImageBuffer::from_pixel(2, 2, Rgb([0u8, 0, 255]));
        let mut buf = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Tiff,
        )
        .unwrap();
        buf
    }

    fn is_png(data: &[u8]) -> bool {
        data.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10])
    }

    #[test]
    fn png_is_re_encoded_as_png() {
        let out = normalize_to_png(&png_bytes()).unwrap();
        assert!(is_png(&out), "stored bytes must be PNG");
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!((img.width(), img.height()), (2, 2));
    }

    #[test]
    fn jpg_is_stored_as_png() {
        let out = normalize_to_png(&jpg_bytes()).unwrap();
        assert!(is_png(&out), "jpg upload must be stored as PNG");
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!((img.width(), img.height()), (2, 2));
    }

    #[test]
    fn tiff_is_stored_as_png() {
        let out = normalize_to_png(&tiff_bytes()).unwrap();
        assert!(is_png(&out), "tiff upload must be stored as PNG");
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!((img.width(), img.height()), (2, 2));
    }

    #[test]
    fn float_tiff_is_rejected() {
        let img = image::ImageBuffer::from_pixel(2, 2, image::Rgb([1.0f32, 0.0, 0.0]));
        let mut buf = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Tiff,
        )
        .unwrap();
        let err = normalize_to_png(&buf).unwrap_err();
        assert!(err.to_string().contains("Float"), "unexpected error: {err}");
    }

    #[test]
    fn invalid_data_is_rejected() {
        assert!(normalize_to_png(b"not an image").is_err());
    }
}
