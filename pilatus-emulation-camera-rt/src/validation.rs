use anyhow::anyhow;
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

/// Checks that the data contains a decodable image
pub(super) fn validate_image(data: &[u8]) -> Result<(), anyhow::Error> {
    image::load_from_memory(data)
        .map(|_| ())
        .map_err(|e| anyhow!("File is not a valid image: {e}"))
}
