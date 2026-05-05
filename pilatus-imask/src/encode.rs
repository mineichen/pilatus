use std::ops::RangeInclusive;

use imask::{ImageDimension, WithRoi};
use pilatus_engineering::image::{AnyMultiMap, EncodeExtension};

pub(super) fn encode_extension() -> EncodeExtension {
    EncodeExtension {
        kind: super::EXTENSION_KIND_MASK,
        writer: Box::new(encode),
    }
}
pub fn encode(extensions: &AnyMultiMap, buf: &mut Vec<u8>) -> std::io::Result<()> {
    for mask in extensions.iter::<imask::SortedRanges<u64, u64>>() {
        buf.push(super::EXTENSION_KIND_MASK);
        //println!("{}", buf.len());
        let roi = mask.bounds();
        let buf2 = &mut *buf;
        futures::executor::block_on(imask::AsyncRangeWriter::new(
            buf2,
            WithRoi::new(
                futures::stream::iter(mask.iter_roi::<RangeInclusive<u64>>()),
                roi,
            ),
        ))?;
        buf.extend_from_slice(&super::MASK_SENTINEL);
    }
    Ok(())
}
