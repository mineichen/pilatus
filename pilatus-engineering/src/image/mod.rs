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
    borrow::Cow,
    fmt::{self, Debug, Formatter},
    mem::ManuallyDrop,
    num::{NonZero, NonZeroU32, TryFromIntError},
    ops::Deref,
    sync::Arc,
};

use crate::{InvertibleTransform, InvertibleTransform3d};

#[cfg(feature = "tokio")]
mod broadcaster;
mod keys;
#[cfg(feature = "image-algorithm")]
mod logo;
mod message;
#[cfg(feature = "image-algorithm")]
mod png;
mod stable_hash;

#[cfg(feature = "tokio")]
pub use broadcaster::*;
use image::GenericImageView;
pub use keys::*;
#[cfg(feature = "image-algorithm")]
pub use logo::*;

pub use message::*;
#[cfg(feature = "image-algorithm")]
pub use png::*;
pub use stable_hash::*;

pub trait PointProjector {
    fn project_to_world_plane(
        &self,
        transform: &InvertibleTransform,
    ) -> Result<InvertibleTransform3d, anyhow::Error>;
}

pub type DynamicPointProjector = Arc<dyn PointProjector + 'static + Send + Sync>;

pub type LumaImage = GenericImage<u8, 1>;

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
pub struct PackedGenericImage(GenericImage<u8, 3>);
impl Deref for PackedGenericImage {
    type Target = GenericImage<u8, 3>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PackedGenericImage {
    pub fn new(i: GenericImage<u8, 3>) -> Self {
        Self(i)
    }
    fn from_unpacked_image(i: &GenericImage<u8, 3>) -> Self {
        let (width, height) = i.dimensions();
        let area = (width.get() * height.get()) as usize;
        let (r, rest) = i.buffer().split_at(area);
        let (g, b) = rest.split_at(area);
        Self::from_unpacked([r, g, b], (width, height))
    }

    pub fn from_unpacked([r, g, b]: [&[u8]; 3], (width, height): (NonZeroU32, NonZeroU32)) -> Self {
        let len = width.get() as usize * height.get() as usize;
        assert_eq!(len, r.len());
        assert_eq!(len, g.len());
        assert_eq!(len, b.len());

        let mut write_buf_container = Arc::new_uninit_slice(len * 3);
        let write_buf = Arc::get_mut(&mut write_buf_container).unwrap();
        let mut next_write = 0;

        for channel in 0..len {
            unsafe {
                write_buf
                    .get_unchecked_mut(next_write)
                    .write(*r.get_unchecked(channel));
                write_buf
                    .get_unchecked_mut(next_write + 1)
                    .write(*g.get_unchecked(channel));
                write_buf
                    .get_unchecked_mut(next_write + 2)
                    .write(*b.get_unchecked(channel));
            }
            next_write += 3;
        }
        PackedGenericImage(GenericImage::<u8, 3>::new_arc(
            unsafe { write_buf_container.assume_init() },
            width,
            height,
        ))
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
        Arc::new(UnpackedGenericImage::from_packed_image(&self))
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
pub struct UnpackedGenericImage(GenericImage<u8, 3>);

impl UnpackedGenericImage {
    pub fn new(i: GenericImage<u8, 3>) -> Self {
        Self(i)
    }

    fn from_packed_image(i: &GenericImage<u8, 3>) -> Self {
        let (width, height) = i.dimensions();
        Self::from_packed(i.buffer(), (width, height))
    }

    pub fn from_packed(v: &[u8], (width, height): (NonZeroU32, NonZeroU32)) -> Self {
        let len = width.get() as usize * height.get() as usize;
        let mut write_buf_container = Arc::new_uninit_slice(len * 3);
        let write_buf = Arc::get_mut(&mut write_buf_container).unwrap();
        let mut next_read = 0;

        let area = (width.get() * height.get()) as usize;
        let twice_area = area + area;

        for channel in 0..len {
            unsafe {
                write_buf
                    .get_unchecked_mut(channel)
                    .write(*v.get_unchecked(next_read));
                write_buf
                    .get_unchecked_mut(channel + area)
                    .write(*v.get_unchecked(next_read + 1));
                write_buf
                    .get_unchecked_mut(channel + twice_area)
                    .write(*v.get_unchecked(next_read + 2));
            }
            next_read += 3;
        }
        UnpackedGenericImage(GenericImage::<u8, 3>::new_arc(
            unsafe { write_buf_container.assume_init() },
            width,
            height,
        ))
    }
}

impl Deref for UnpackedGenericImage {
    type Target = GenericImage<u8, 3>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl RgbImage for UnpackedGenericImage {
    fn is_packed(&self) -> bool {
        false
    }

    fn into_packed(self: Arc<Self>) -> Arc<dyn PackedRgbImage> {
        Arc::new(PackedGenericImage::from_unpacked_image(&self))
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
                std::slice::from_raw_parts(self.ptr, offset as usize),
                std::slice::from_raw_parts(self.ptr.offset(offset), offset as usize),
                std::slice::from_raw_parts(self.ptr.offset(offset * 2), offset as usize),
            ]
        }
    }
}

impl From<&GenericImage<u8, 1>> for PackedGenericImage {
    fn from(input: &GenericImage<u8, 1>) -> Self {
        let data = input.buffer().iter().flat_map(|&i| [i, i, i]).collect();
        let inner = GenericImage::<u8, 3>::new_arc(data, input.width, input.height);
        PackedGenericImage(inner)
    }
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum DynamicImage {
    Luma8(LumaImage),
    Luma16(GenericImage<u16, 1>),
    /// r,r,r,r,g,g,g,g,b,b,b,b
    Rgb8Planar(GenericImage<u8, 3>),
}

impl DynamicImage {
    pub fn dimensions(&self) -> (NonZero<u32>, NonZero<u32>) {
        match self {
            DynamicImage::Luma8(x) => x.dimensions(),
            DynamicImage::Luma16(x) => x.dimensions(),
            DynamicImage::Rgb8Planar(x) => x.dimensions(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImageConversionError {
    #[error("Dimensions mustn't be 0")]
    InvalidSize(#[from] TryFromIntError),

    #[error("Unsupported format {0}")]
    Unsupported(Cow<'static, str>),
}

impl TryFrom<image::DynamicImage> for DynamicImage {
    type Error = ImageConversionError;

    fn try_from(value: image::DynamicImage) -> Result<Self, Self::Error> {
        let (width, height) = value.dimensions();
        let (width, height) = (width.try_into()?, height.try_into()?);
        match value {
            image::DynamicImage::ImageLuma8(x) => Ok(DynamicImage::Luma8(GenericImage::new_vec(
                x.into_vec(),
                width,
                height,
            ))),
            image::DynamicImage::ImageLuma16(x) => Ok(DynamicImage::Luma16(GenericImage::new_vec(
                x.into_vec(),
                width,
                height,
            ))),
            image::DynamicImage::ImageLumaA8(_) => Err(ImageConversionError::Unsupported(
                Cow::Borrowed("ImageLumaA8"),
            )),
            image::DynamicImage::ImageRgb8(x) => {
                let unpacked = UnpackedGenericImage::from_packed_image(&PackedGenericImage(
                    GenericImage::new_vec(x.into_vec(), width, height),
                ));
                Ok(DynamicImage::Rgb8Planar(unpacked.0))
            }
            image::DynamicImage::ImageRgba8(_) => Err(ImageConversionError::Unsupported(
                Cow::Borrowed("ImageRgba8"),
            )),
            image::DynamicImage::ImageLumaA16(_) => Err(ImageConversionError::Unsupported(
                Cow::Borrowed("ImageLumaA16"),
            )),
            image::DynamicImage::ImageRgb16(_) => Err(ImageConversionError::Unsupported(
                Cow::Borrowed("ImageRgb16"),
            )),
            image::DynamicImage::ImageRgba16(_) => Err(ImageConversionError::Unsupported(
                Cow::Borrowed("ImageRgba16"),
            )),
            image::DynamicImage::ImageRgb32F(_) => Err(ImageConversionError::Unsupported(
                Cow::Borrowed("ImageRgb32F"),
            )),
            image::DynamicImage::ImageRgba32F(_) => Err(ImageConversionError::Unsupported(
                Cow::Borrowed("ImageRgba32F"),
            )),
            _ => Err(ImageConversionError::Unsupported(Cow::Borrowed("Unknown"))),
        }
    }
}

#[repr(C)]
pub struct GenericImage<T: 'static, const CHANNELS: usize> {
    pub ptr: *const T,
    width: NonZeroU32,
    height: NonZeroU32,

    vtable: &'static ImageVtable<T, CHANNELS>,

    // Has to be cleaned up by clear proc too
    pub data: usize,
}

impl<T, const CHANNELS: usize> Clone for GenericImage<T, CHANNELS> {
    fn clone(&self) -> Self {
        unsafe { (self.vtable.clone)(self) }
    }
}

impl<T: std::cmp::PartialEq, const CHANNELS: usize> PartialEq for GenericImage<T, CHANNELS> {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width && self.height == other.height && self.buffer() == other.buffer()
    }
}

#[repr(C)]
#[derive(PartialEq, PartialOrd, Eq, Ord)]
pub struct ImageVtable<T: 'static, const CHANNELS: usize> {
    pub clone: unsafe extern "C" fn(&GenericImage<T, CHANNELS>) -> GenericImage<T, CHANNELS>,
    pub make_mut:
        unsafe extern "C" fn(&mut GenericImage<T, CHANNELS>, out_len: &mut usize) -> *mut T,
    pub drop: unsafe extern "C" fn(&mut GenericImage<T, CHANNELS>),
}

extern "C" fn clear_vec<T, const CHANNELS: usize>(image: &mut GenericImage<T, CHANNELS>) {
    unsafe {
        Vec::from_raw_parts(
            image.ptr as *mut u8,
            (image.width.get() * image.height.get()) as usize * CHANNELS,
            image.data,
        )
    };
}
extern "C" fn clone_slice<T: Clone, const CHANNELS: usize>(
    image: &GenericImage<T, CHANNELS>,
) -> GenericImage<T, CHANNELS> {
    GenericImage::new_arc(Arc::from(image.buffer()), image.width, image.height)
}

impl<TP: std::any::Any, const CHANNELS: usize> Debug for GenericImage<TP, CHANNELS> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenericImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("channels", &CHANNELS)
            .field("pixel", &std::any::type_name::<TP>())
            .finish()
    }
}

unsafe impl<TP: Send, const T: usize> Send for GenericImage<TP, T> {}
unsafe impl<TP: Sync, const T: usize> Sync for GenericImage<TP, T> {}

impl<'a> From<&'a LumaImage> for (&'a [u8], NonZeroU32, NonZeroU32) {
    fn from(that: &'a LumaImage) -> Self {
        let (width, height) = that.dimensions();
        let buf = that.buffer();
        (buf, width, height)
    }
}

// Workaroung inability to have static which uses Outer Generics
trait Factory<T: 'static, const CHANNELS: usize> {
    const VTABLE: &'static ImageVtable<T, CHANNELS>;
}
struct VecFactory;

impl<T: 'static + Clone, const CHANNELS: usize> Factory<T, CHANNELS> for VecFactory {
    const VTABLE: &'static ImageVtable<T, CHANNELS> = {
        unsafe extern "C" fn make_mut<T: Clone, const CHANNELS: usize>(
            image: &mut GenericImage<T, CHANNELS>,
            out_len: &mut usize,
        ) -> *mut T {
            *out_len = image.len();
            image.ptr as *mut T
        }
        &ImageVtable {
            make_mut,
            drop: clear_vec,
            clone: clone_slice,
        }
    };
}

struct ArcFactory;

impl<T: 'static + Clone, const CHANNELS: usize> Factory<T, CHANNELS> for ArcFactory {
    const VTABLE: &'static ImageVtable<T, CHANNELS> = {
        unsafe extern "C" fn make_mut<T: Clone, const CHANNELS: usize>(
            image: &mut GenericImage<T, CHANNELS>,
            out_len: &mut usize,
        ) -> *mut T {
            let mut arc = ManuallyDrop::new(unsafe {
                let ptr = std::ptr::slice_from_raw_parts(image.ptr, image.data);
                Arc::<[T]>::from_raw(ptr)
            });

            let ptr;
            (ptr, *out_len) = if let Some(ptr) = Arc::get_mut(&mut arc) {
                (ptr.as_mut_ptr(), ptr.len())
            } else {
                let mut new_data = Arc::<[T]>::from(&arc[..]);
                ManuallyDrop::into_inner(arc);

                let ptr = Arc::get_mut(&mut new_data).expect("Just created, must be unique");
                let r = (ptr.as_mut_ptr(), ptr.len());
                image.ptr = Arc::into_raw(new_data).cast::<T>();
                r
            };
            ptr
        }
        extern "C" fn clear_arc<T: Clone, const CHANNELS: usize>(
            image: &mut GenericImage<T, CHANNELS>,
        ) {
            unsafe {
                let ptr = std::ptr::slice_from_raw_parts(image.ptr, image.data);
                Arc::<[T]>::from_raw(ptr);
            }
        }

        extern "C" fn clone_arc<T: Clone, const CHANNELS: usize>(
            image: &GenericImage<T, CHANNELS>,
        ) -> GenericImage<T, CHANNELS> {
            let arc = ManuallyDrop::new(unsafe {
                let ptr = std::ptr::slice_from_raw_parts(image.ptr, image.data);
                Arc::<[T]>::from_raw(ptr)
            });
            GenericImage::new_arc((*arc).clone(), image.width, image.height)
        }

        &ImageVtable {
            drop: clear_arc,
            clone: clone_arc,
            make_mut,
        }
    };
}

#[allow(clippy::len_without_is_empty)]
impl<const CHANNELS: usize, T: 'static> GenericImage<T, CHANNELS> {
    #[deprecated = "Use eigher new_vec or new_arc"]
    pub fn new(input: Vec<T>, width: NonZeroU32, height: NonZeroU32) -> Self
    where
        T: Clone,
    {
        Self::new_vec(input, width, height)
    }

    pub fn new_vec(input: Vec<T>, width: NonZeroU32, height: NonZeroU32) -> Self
    where
        T: Clone,
    {
        let cap = input.capacity();
        let buf = input.as_ptr();
        assert_eq!(
            input.len() as u32,
            width.get() * height.get() * CHANNELS as u32,
            "Incompatible Buffer-Size"
        );

        std::mem::forget(input);
        let vtable = <VecFactory as Factory<T, CHANNELS>>::VTABLE;
        unsafe { Self::new_with_vtable(buf, width, height, vtable, cap) }
    }

    pub fn new_arc(input: Arc<[T]>, width: NonZeroU32, height: NonZeroU32) -> Self
    where
        T: Clone,
    {
        assert_eq!(
            input.len() as u32,
            width.get() * height.get() * CHANNELS as u32,
            "Incompatible Buffer-Size"
        );

        let len = input.len();
        let input = Arc::into_raw(input);
        let vtable = <ArcFactory as Factory<T, CHANNELS>>::VTABLE;
        unsafe { Self::new_with_vtable(input.cast::<T>(), width, height, vtable, len) }
    }
    pub const fn len(&self) -> usize {
        self.width.get() as usize * self.height.get() as usize * CHANNELS
    }

    pub const fn buffer(&self) -> &[T] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len()) }
    }

    pub fn make_mut(&mut self) -> &mut [T] {
        unsafe {
            let mut len = 0;
            let ptr = (self.vtable.make_mut)(self, &mut len);
            std::slice::from_raw_parts_mut(ptr, len)
        }
    }

    /// Don't use this method unless you need a custom image.
    ///
    /// Use/provide methods like new_vec() and new_arc() for safe construction
    ///
    /// # Safety
    /// The vtable must be able to cleanup the fields
    pub unsafe fn new_with_vtable(
        buf: *const T,
        width: NonZeroU32,
        height: NonZeroU32,
        vtable: &'static ImageVtable<T, CHANNELS>,
        generic_field: usize,
    ) -> Self {
        assert!(matches!(CHANNELS, 1 | 3 | 4));

        Self {
            ptr: buf,
            width,
            height,
            vtable,
            data: generic_field,
        }
    }

    pub fn to_vec(self) -> Vec<T>
    where
        T: Clone,
    {
        if self.vtable.drop as usize == clear_vec::<T, CHANNELS> as usize {
            let size = self.len();
            let result = unsafe { Vec::from_raw_parts(self.ptr as *mut _, size, self.data) };
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

impl<T, const CHANNELS: usize> Drop for GenericImage<T, CHANNELS> {
    fn drop(&mut self) {
        if self.ptr as usize != 0 {
            unsafe { (self.vtable.drop)(self) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn miri_create_and_clear_vec_image() {
        let size = 2.try_into().unwrap();
        let image = LumaImage::new_vec(vec![0u8, 64u8, 128u8, 192u8], size, size);
        assert_eq!(image.buffer(), &[0u8, 64u8, 128u8, 192u8]);
    }
    #[test]
    fn miri_to_vec_reuses_pointer() {
        let raw = vec![0u8, 64u8, 128u8, 192u8];
        let pointer = raw[..].as_ptr();
        let size = 2.try_into().unwrap();
        let image = LumaImage::new_vec(raw, size, size);
        let to_vec = image.to_vec();

        // Miri seems to generate clear_vec::<const u8> for each call
        // It works on native x86. Because it's only an optimization, this is good enough
        // VTable is not possible, as GenericImage is ABI-Stable and multiple dylibs use their own allocator for Vecs
        if !cfg!(miri) {
            assert_eq!(
                to_vec[..].as_ptr(),
                pointer,
                "Should reuse the buffer if it was created by vec"
            );
        }
    }

    #[test]
    fn miri_make_mut_reuses_arc_pointer() {
        let raw = Arc::<[u8]>::from([0u8, 64u8, 128u8, 192u8].as_slice());
        let pointer = raw[..].as_ptr();
        let size = 2.try_into().unwrap();
        let mut image = LumaImage::new_arc(raw, size, size);
        let ptr_mut = image.make_mut();

        assert_eq!(
            ptr_mut[..].as_ptr(),
            pointer,
            "Should reuse the buffer if it was created by vec"
        );
    }

    #[test]
    fn miri_make_mut_doesnt_reuse_arc_pointer_if_not_unique() {
        let raw = Arc::<[u8]>::from([0u8, 64u8, 128u8, 192u8].as_slice());
        let _raw2 = raw.clone();
        let pointer = raw[..].as_ptr();
        let size = 2.try_into().unwrap();
        let mut image = LumaImage::new_arc(raw, size, size);
        let ptr_mut = image.make_mut();

        assert_ne!(
            ptr_mut[..].as_ptr(),
            pointer,
            "Should reuse the buffer if it was created by vec"
        );
    }

    #[test]
    fn miri_clone_arc_backed_shares_memory() {
        let raw = Arc::<[u8]>::from([0u8, 64u8, 128u8, 192u8].as_slice());
        let pointer = raw[..].as_ptr();
        let size = 2.try_into().unwrap();
        let image = LumaImage::new_arc(raw, size, size);
        let image2 = image.clone();

        assert_eq!(
            image2.buffer().as_ptr(),
            pointer,
            "Should reuse the buffer if it was created by vec"
        );
    }

    #[test]
    fn miri_clone_from_box() {
        let raw = vec![0u8, 64u8, 128u8, 192u8];
        let size = 2.try_into().unwrap();
        let image = LumaImage::new_vec(raw, size, size);
        let image2 = image.clone();
        let to_vec = image.to_vec();
        let to_vec2 = image2.to_vec();

        assert_ne!(
            to_vec[..].as_ptr(),
            to_vec2[..].as_ptr(),
            "Should reuse the buffer if it was created by vec"
        );
    }

    #[test]
    fn miri_test_into_packed() {
        let size = 2.try_into().unwrap();
        let image = Arc::new(UnpackedGenericImage(GenericImage::<u8, 3>::new_vec(
            vec![1u8, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3],
            size,
            size,
        )));
        let packed = image.into_packed().into_vec();
        assert_eq!(packed.to_vec(), vec!(1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3));
    }
}
