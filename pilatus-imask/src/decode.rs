use std::{io, num::NonZero};

use imask::{AsyncRangeStream, ImaskSet, SortedRanges};
use pilatus_engineering::image::{AnyMultiMap, DecodeExtension};

pub fn decode_extension() -> DecodeExtension {
    DecodeExtension {
        kind: super::EXTENSION_KIND_MASK,
        reader: Box::new(decode),
    }
}
pub fn decode<'a>(input: &'a [u8], target: &mut AnyMultiMap) -> io::Result<&'a [u8]> {
    let (mask, rest) = decode_mask(input)?;
    target.insert(mask);
    Ok(rest)
}

fn decode_mask(input: &[u8]) -> io::Result<(SortedRanges<u64, u64>, &[u8])> {
    let mut err = io::Result::Ok(());
    let invalid_data = |msg: &str| io::Error::new(io::ErrorKind::InvalidData, msg);
    let end_pos = input
        .array_windows()
        .position(|x| x == &super::MASK_SENTINEL)
        .ok_or_else(|| invalid_data("Sentinel for RangeEnd not found"))?;
    let (ranges, rest) = input.split_at(end_pos);

    let stream = futures::executor::block_on(AsyncRangeStream::new(ranges))?;
    let roi = stream.roi();
    let width = NonZero::try_from(roi.offset_x + roi.width.get())
        .map_err(|_| invalid_data("offset_x + width are zero because of overflow"))?;
    let height = NonZero::try_from(roi.offset_y + roi.height.get())
        .map_err(|_| invalid_data("offset_y + height are zero because of overflow"))?;
    Ok((
        imask::SortedRanges::<u64, u64>::try_from_ordered_iter(
            futures::executor::block_on_stream(stream.into_roi_stream())
                .map(|input| match input {
                    Ok(x) => Some(x),
                    Err(e) => {
                        err = Err(e.into());
                        None
                    }
                })
                .take_while(|x| x.is_some())
                .map(|x| x.unwrap())
                .with_bounds(width, height), //.with_roi(rect),
        )
        .map_err(|s| invalid_data(&format!("Cannot create SortedRanges: {s}")))?,
        &rest[super::MASK_SENTINEL.len()..],
    ))
}
