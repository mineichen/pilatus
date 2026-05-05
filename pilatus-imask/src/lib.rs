mod decode;
mod encode;

const EXTENSION_KIND_MASK: u8 = 2 << 4;
// Both dimensions cannot be 0... If we find 15 consecutive zeros, we know it's impossible to be valid ranges
// A valid option for 14 consecutive zero bytes would be the following two u64 values [[1, 0, 0, 0, 0, 0, 0, 0],[0, 0, 0, 0, 0, 0, 0, 1]]
// the ending 42 is there, to make sure, we don't cut numbers which still belong to a number (u64 + sentinel) [[0, 0, 0, 0, 0, 0, 1, 0], [0,0,0...]]
const MASK_SENTINEL: [u8; 16] = {
    let mut sentinel = [0; 16];
    sentinel[15] = 42;
    sentinel
};

pub fn register_services(c: &mut minfac::ServiceCollection) {
    c.register(encode::encode_extension);
    c.register(decode::decode_extension);
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU32, sync::Arc};

    use imask::{ImaskSet, Rect, SortedRanges};
    use imbuf::Image;
    use pilatus_engineering::image::{
        DecodeExtensions, EncodeExtensions, ImageWithMeta, MetaImageDecoder, MetaImageEncoder,
        StreamingImageFormat,
    };
    use testresult::TestResult;

    use super::*;

    #[test]
    fn encode_decode_with_imask() -> TestResult {
        type Ranges = SortedRanges<u64, u64>;
        let image = Image::<u8, 1>::new_vec_flat(vec![128], NonZeroU32::MIN, NonZeroU32::MIN);
        let width = const { NonZeroU32::new(10).unwrap() };
        let height = const { NonZeroU32::new(20).unwrap() };
        let roi = Rect::new(100, 100, width, height);
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

        let encoder = MetaImageEncoder::with_extensions(Arc::new(EncodeExtensions::new(vec![
            encode::encode_extension(),
        ])));
        let bytes = encoder.encode(encodable_image, StreamingImageFormat::Raw)?;
        let decoder = MetaImageDecoder::with_extensions(Arc::new(DecodeExtensions::new(vec![
            decode::decode_extension(),
        ])));
        let back = decoder
            .decode(&bytes)??
            .try_convert_image::<Image<u8, 1>>()?;

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
