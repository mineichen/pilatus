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
    fmt::{self, Debug, Formatter},
    num::NonZeroU32,
    ops::Deref,
    sync::Arc,
};

use crate::{InvertibleTransform, InvertibleTransform3d};

#[cfg(feature = "tokio")]
mod broadcaster;
#[cfg(feature = "image-algorithm")]
mod logo;
mod message;
mod stable_hash;

#[cfg(feature = "tokio")]
pub use broadcaster::*;
#[cfg(feature = "image-algorithm")]
pub use logo::*;
pub use message::*;
pub use stable_hash::*;

#[cfg(feature = "image-algorithm")]
pub(super) fn register_services(c: &mut minfac::ServiceCollection) {
    logo::register_services(c);
}

pub trait PointProjector {
    fn project_to_world_plane(
        &self,
        transform: &InvertibleTransform,
    ) -> Result<InvertibleTransform3d, anyhow::Error>;
}

pub type DynamicPointProjector = Arc<dyn PointProjector + 'static + Send + Sync>;

pub type LumaImage = GenericImage<1>;

pub trait RgbImage: Debug {
    fn is_packed(&self) -> bool;
    fn size(&self) -> (NonZeroU32, NonZeroU32);
    /// Returns a buffer with layout RGBRGBRGBRGB
    fn into_packed(self: Arc<Self>) -> Arc<dyn PackedRgbImage>;
    /// Returns a buffer with layout RRRRGGGGBBBB
    fn into_unpacked(self: Arc<Self>) -> Arc<dyn UnpackedRgbImage>;
}

/// All bytes are stored in a continuous buffer (y:x:channel)
pub trait PackedRgbImage: RgbImage {
    fn buffer(&self) -> &[u8];
    fn into_vec(self: Arc<Self>) -> Vec<u8>;
}

/// Image consists of one image per channel (channel:y:x)
/// Channels mustn't be continuous in memory
pub trait UnpackedRgbImage {
    fn get_channels(&self) -> [&[u8]; 3];
}

#[derive(Debug)]
pub struct PackedGenericImage(GenericImage<3>);
impl Deref for PackedGenericImage {
    type Target = GenericImage<3>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PackedGenericImage {
    pub fn new(i: GenericImage<3>) -> Self {
        Self(i)
    }
    pub fn from_unpacked([r, g, b]: [&[u8]; 3], (width, height): (NonZeroU32, NonZeroU32)) -> Self {
        let len = width.get() as usize * height.get() as usize;
        assert_eq!(len, r.len());
        assert_eq!(len, g.len());
        assert_eq!(len, b.len());

        let mut write_buf = vec![0; len * 3];
        let mut next_write = 0;

        for channel in 0..len {
            unsafe {
                *write_buf.get_unchecked_mut(next_write) = *r.get_unchecked(channel);
                *write_buf.get_unchecked_mut(next_write + 1) = *g.get_unchecked(channel);
                *write_buf.get_unchecked_mut(next_write + 2) = *b.get_unchecked(channel);
            }
            next_write += 3;
        }
        PackedGenericImage(GenericImage::<3>::new(write_buf, width, height))
    }
}

impl RgbImage for PackedGenericImage {
    fn is_packed(&self) -> bool {
        true
    }

    fn into_packed(self: Arc<Self>) -> Arc<dyn PackedRgbImage> {
        self
    }

    fn into_unpacked(self: Arc<Self>) -> Arc<dyn UnpackedRgbImage> {
        unimplemented!()
    }

    fn size(&self) -> (NonZeroU32, NonZeroU32) {
        self.dimensions()
    }
}

impl PackedRgbImage for PackedGenericImage {
    fn into_vec(self: Arc<Self>) -> Vec<u8> {
        match Arc::try_unwrap(self) {
            Ok(inner) => inner.0.to_vec(),
            Err(shared_self) => shared_self.buffer().to_vec(),
        }
    }

    fn buffer(&self) -> &[u8] {
        self.0.buffer()
    }
}

#[derive(Debug)]
pub struct UnpackedGenericImage(GenericImage<3>);

impl UnpackedGenericImage {
    pub fn new(i: GenericImage<3>) -> Self {
        Self(i)
    }
}

impl Deref for UnpackedGenericImage {
    type Target = GenericImage<3>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl RgbImage for UnpackedGenericImage {
    fn is_packed(&self) -> bool {
        false
    }

    fn into_packed(self: Arc<Self>) -> Arc<dyn PackedRgbImage> {
        let (width, height) = self.dimensions();
        let area = (width.get() * height.get()) as usize;
        let (r, rest) = self.buffer().split_at(area);
        let (g, b) = rest.split_at(area);
        Arc::new(PackedGenericImage::from_unpacked(
            [r, g, b],
            (width, height),
        ))
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
        let offset = (self.width.get() * self.height.get()) as isize;

        unsafe {
            [
                std::slice::from_raw_parts(self.buf, offset as usize),
                std::slice::from_raw_parts(self.buf.offset(offset), offset as usize),
                std::slice::from_raw_parts(self.buf.offset(offset * 2), offset as usize),
            ]
        }
    }
}

impl<'a> From<&'a GenericImage<1>> for PackedGenericImage {
    fn from(input: &GenericImage<1>) -> Self {
        let data = input.buffer().iter().flat_map(|&i| [i, i, i]).collect();
        let inner = GenericImage::<3>::new(data, input.width, input.height);
        PackedGenericImage(inner)
    }
}

#[repr(C)]
pub struct GenericImage<const CHANNELS: usize> {
    buf: *const u8,
    width: NonZeroU32,
    height: NonZeroU32,

    clear_proc: extern "C" fn(&mut GenericImage<CHANNELS>, usize),
    // Has to be cleaned up by clear proc too
    generic_field: usize,
}

impl<const CHANNELS: usize> Debug for GenericImage<CHANNELS> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenericImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("dept", &CHANNELS)
            .finish()
    }
}

impl<const CHANNELS: usize> Clone for GenericImage<CHANNELS> {
    fn clone(&self) -> Self {
        let (width, height) = self.dimensions();
        let buf = self.buffer().to_vec();
        GenericImage::new(buf, width, height)
    }
}

unsafe impl<const T: usize> Send for GenericImage<T> {}
unsafe impl<const T: usize> Sync for GenericImage<T> {}

extern "C" fn clear_vec<const CHANNELS: usize>(
    image: &mut GenericImage<CHANNELS>,
    generic_field: usize,
) {
    unsafe {
        Vec::from_raw_parts(
            image.buf as *mut u8,
            (image.width.get() * image.height.get()) as usize,
            generic_field,
        )
    };
}

impl<'a> From<&'a LumaImage> for (&'a [u8], NonZeroU32, NonZeroU32) {
    fn from(that: &'a LumaImage) -> Self {
        let (width, height) = that.dimensions();
        let buf = that.buffer();
        (buf, width, height)
    }
}

impl<const CHANNELS: usize> GenericImage<CHANNELS> {
    pub fn new(input: Vec<u8>, width: NonZeroU32, height: NonZeroU32) -> Self {
        let cap = input.capacity();
        let buf = input.as_ptr();
        assert_eq!(
            input.len() as u32,
            width.get() * height.get() * CHANNELS as u32,
            "Incompatible Buffer-Size"
        );

        std::mem::forget(input);
        unsafe { Self::new_with_cleanup(buf, width, height, clear_vec::<CHANNELS>, cap) }
    }

    fn buffer_size(&self) -> usize {
        self.width.get() as usize * self.height.get() as usize * CHANNELS
    }

    pub fn buffer(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.buf, self.buffer_size()) }
    }

    /// # Safety
    ///
    /// The buffer must point to allocated memory of size width*height*CHANNELS.
    /// Ownership of this buffer should logically be transferred to this image.
    /// generic_field can be used to store e.g. pointers to CPP-Objects which
    /// should be freed in the clear_proc
    pub unsafe fn new_with_cleanup(
        buf: *const u8,
        width: NonZeroU32,
        height: NonZeroU32,
        clear_proc: extern "C" fn(&mut Self, usize),
        generic_field: usize,
    ) -> Self {
        assert!(matches!(CHANNELS, 1 | 3 | 4));

        Self {
            buf,
            width,
            height,
            clear_proc,
            generic_field,
        }
    }

    pub fn to_vec(self) -> Vec<u8> {
        if self.clear_proc as usize == clear_vec::<CHANNELS> as usize {
            let size = self.buffer_size();
            let result = unsafe { Vec::from_raw_parts(self.buf as *mut _, size, self.generic_field) };
            std::mem::forget(self);
            result
        } else {
            self.buffer().to_vec()
        }
    }
    pub fn dimensions(&self) -> (NonZeroU32, NonZeroU32) {
        (self.width, self.height)
    }
}

impl<const CHANNELS: usize> Drop for GenericImage<CHANNELS> {
    fn drop(&mut self) {
        if self.buf as usize != 0 {
            let generic_field = self.generic_field;
            (self.clear_proc)(self, generic_field);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn miri_create_and_clear_vec_image() {
        let size = 2.try_into().unwrap();
        let image = LumaImage::new(vec![0u8, 64u8, 128u8, 192u8], size, size);
        assert_eq!(image.buffer(), &[0u8, 64u8, 128u8, 192u8]);
    }
    #[test]
    fn miri_to_vec_reuses_pointer() {
        let raw = vec![0u8, 64u8, 128u8, 192u8];
        let pointer = raw[..].as_ptr();
        let size = 2.try_into().unwrap();
        let image = LumaImage::new(raw, size, size);
        let to_vec = image.to_vec();

        // Miri seems to generate clear_vec::<const u8> for each call
        // It works on native x86. Because it's only an optimization, this is good enough
        if !cfg!(miri) {
            assert_eq!(
                to_vec[..].as_ptr(),
                pointer,
                "Should reuse the buffer if it was created by vec"
            );
        }
    }

    #[test]
    fn miri_test_into_packed() {
        let raw = vec![1u8, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3];

        let mut input = raw;
        let cap = input.capacity();
        let pointer = input[..].as_mut_ptr();
        std::mem::forget(input);
        let size = 2.try_into().unwrap();
        let image = Arc::new(UnpackedGenericImage(unsafe {
            GenericImage::<3>::new_with_cleanup(pointer, size, size, clear_vec::<3>, cap)
        }));
        let packed = image.into_packed().into_vec();
        assert_eq!(packed.to_vec(), vec!(1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3));
    }
}
