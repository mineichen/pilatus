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
    sync::Arc,
};

use crate::{InvertibleTransform, InvertibleTransform3d};

#[cfg(feature = "tokio")]
mod broadcaster;
#[cfg(feature = "image-algorithm")]
mod logo;
mod message;
mod meta;
#[cfg(feature = "image-algorithm")]
mod png;
mod stable_hash;

#[cfg(feature = "tokio")]
pub use broadcaster::*;
use image::GenericImageView;
#[cfg(feature = "image-algorithm")]
pub use logo::*;
pub use meta::*;

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
    fn flat_buffer(&self) -> &[u8];
    //fn into_flat_vec(self: Arc<Self>) -> Vec<u8>;
}

/// Image consists of one image per channel (channel:y:x)
/// Channels mustn't be continuous in memory
pub trait UnpackedRgbImage {
    fn get_channels(&self) -> [&[u8]; 3];
}

pub type PackedGenericImage = GenericImage<[u8; 3], 1>;

impl PackedGenericImage {
    pub fn from_unpacked_image(i: &GenericImage<u8, 3>) -> Self {
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

        GenericImage::<[u8; 3], 1>::new_arc(
            r.iter()
                .zip(g)
                .zip(b)
                .map(|((&r, &g), &b)| [r, g, b])
                .collect(),
            width,
            height,
        )
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

impl PackedRgbImage for GenericImage<[u8; 3], 1> {
    fn flat_buffer(&self) -> &[u8] {
        // SAFETY: [u8; 3] has the same layout as 3 consecutive u8 values
        unsafe { std::slice::from_raw_parts(self.buffer().as_ptr() as *const u8, self.len() * 3) }
    }
}

pub type UnpackedGenericImage = GenericImage<u8, 3>;

impl From<UnpackedGenericImage> for PackedGenericImage {
    fn from(value: UnpackedGenericImage) -> Self {
        PackedGenericImage::from_unpacked_image(&value)
    }
}

impl From<PackedGenericImage> for DynamicImage {
    fn from(value: PackedGenericImage) -> Self {
        let planar = UnpackedGenericImage::from_packed_image(&value);
        DynamicImage::Rgb8Planar(planar)
    }
}

impl UnpackedGenericImage {
    fn from_packed_image(i: &GenericImage<[u8; 3], 1>) -> Self {
        let (width, height) = i.dimensions();
        Self::from_flat_packed(i.flat_buffer(), (width, height))
    }

    pub fn from_flat_packed(v: &[u8], (width, height): (NonZeroU32, NonZeroU32)) -> Self {
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
        GenericImage::<u8, 3>::new_arc(unsafe { write_buf_container.assume_init() }, width, height)
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
        let offset = (self.0.width.get() * self.0.height.get()) as isize;

        unsafe {
            [
                std::slice::from_raw_parts(self.0.ptr, offset as usize),
                std::slice::from_raw_parts(self.0.ptr.offset(offset), offset as usize),
                std::slice::from_raw_parts(self.0.ptr.offset(offset * 2), offset as usize),
            ]
        }
    }
}

impl From<&GenericImage<u8, 1>> for PackedGenericImage {
    fn from(input: &GenericImage<u8, 1>) -> Self {
        let data = input.buffer().iter().map(|&i| [i, i, i]).collect();
        GenericImage::<[u8; 3], 1>::new_arc(data, input.0.width, input.0.height)
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

impl From<LumaImage> for DynamicImage {
    fn from(value: LumaImage) -> Self {
        DynamicImage::Luma8(value)
    }
}

impl From<GenericImage<u16, 1>> for DynamicImage {
    fn from(value: GenericImage<u16, 1>) -> Self {
        DynamicImage::Luma16(value)
    }
}

impl TryFrom<DynamicImage> for GenericImage<u16, 1> {
    type Error = UnsupportedImageError<DynamicImage>;

    fn try_from(value: DynamicImage) -> Result<Self, Self::Error> {
        if let DynamicImage::Luma16(x) = value {
            Ok(x)
        } else {
            let msg = format!("{value:?}");
            Err(UnsupportedImageError(value, msg.into()))
        }
    }
}

impl<'a> TryFrom<&'a DynamicImage> for &'a GenericImage<u16, 1> {
    type Error = UnsupportedImageError<&'a DynamicImage>;

    fn try_from(value: &'a DynamicImage) -> Result<Self, Self::Error> {
        if let DynamicImage::Luma16(x) = value {
            Ok(x)
        } else {
            let msg = format!("{value:?}");
            Err(UnsupportedImageError(value, msg.into()))
        }
    }
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
#[error("Unsupported format {1}")]
pub struct UnsupportedImageError<TIn>(TIn, Cow<'static, str>);

#[derive(Debug, thiserror::Error)]
pub enum ImageConversionError {
    #[error("Dimensions mustn't be 0")]
    InvalidSize(#[from] TryFromIntError),

    #[error("{0}")]
    Unsupported(#[from] UnsupportedImageError<image::DynamicImage>),
}

impl TryFrom<image::DynamicImage> for DynamicImage {
    type Error = ImageConversionError;

    fn try_from(value: image::DynamicImage) -> Result<Self, Self::Error> {
        let (width, height) = value.dimensions();
        let (width, height) = (width.try_into()?, height.try_into()?);
        let invalid_format = match value {
            image::DynamicImage::ImageLuma8(x) => {
                return Ok(DynamicImage::Luma8(GenericImage::new_vec(
                    x.into_vec(),
                    width,
                    height,
                )))
            }
            image::DynamicImage::ImageLuma16(x) => {
                return Ok(DynamicImage::Luma16(GenericImage::new_vec(
                    x.into_vec(),
                    width,
                    height,
                )))
            }
            image::DynamicImage::ImageLumaA8(_) => "ImageLumaA8",
            image::DynamicImage::ImageRgb8(x) => {
                let vec_u8 = x.into_vec();
                let unpacked = UnpackedGenericImage::from_flat_packed(&vec_u8, (width, height));
                return Ok(DynamicImage::Rgb8Planar(unpacked));
            }
            image::DynamicImage::ImageRgba8(_) => "ImageRgba8",
            image::DynamicImage::ImageLumaA16(_) => "ImageLumaA16",
            image::DynamicImage::ImageRgb16(_) => "ImageRgb16",
            image::DynamicImage::ImageRgba16(_) => "ImageRgba16",
            image::DynamicImage::ImageRgb32F(_) => "ImageRgb32F",
            image::DynamicImage::ImageRgba32F(_) => "ImageRgba32F",
            _ => "Unknown",
        };
        Err(UnsupportedImageError(value, Cow::Borrowed(invalid_format)).into())
    }
}

#[repr(C)]
pub struct UnsafeGenericImage<T: 'static, const CHANNELS: usize> {
    pub ptr: *const T,
    pub width: NonZeroU32,
    pub height: NonZeroU32,
    pub vtable: &'static ImageVtable<T, CHANNELS>,
    // Has to be cleaned up by clear proc too
    pub data: usize,
}
impl<const CHANNELS: usize, T: 'static> UnsafeGenericImage<T, CHANNELS> {
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

        UnsafeGenericImage {
            ptr: buf,
            width,
            height,
            vtable,
            data: generic_field,
        }
    }
}

#[repr(transparent)]
pub struct GenericImage<T: 'static, const CHANNELS: usize>(UnsafeGenericImage<T, CHANNELS>);

impl<T, const CHANNELS: usize> Clone for GenericImage<T, CHANNELS> {
    fn clone(&self) -> Self {
        Self(unsafe { (self.0.vtable.clone)(&self.0) })
    }
}

// Todo: Fixme, this is not correct
impl<T: std::cmp::PartialEq, const CHANNELS: usize> PartialEq for GenericImage<T, CHANNELS> {
    fn eq(&self, other: &Self) -> bool {
        self.0.width == other.0.width
            && self.0.height == other.0.height
            && self.buffer() == other.buffer()
    }
}

#[repr(C)]
pub struct ImageVtable<T: 'static, const CHANNELS: usize> {
    pub clone:
        unsafe extern "C" fn(&UnsafeGenericImage<T, CHANNELS>) -> UnsafeGenericImage<T, CHANNELS>,
    pub make_mut: unsafe extern "C" fn(&mut UnsafeGenericImage<T, CHANNELS>) -> *mut T,
    pub drop: unsafe extern "C" fn(&mut UnsafeGenericImage<T, CHANNELS>),
}

extern "C" fn clear_vec<T, const CHANNELS: usize>(image: &mut UnsafeGenericImage<T, CHANNELS>) {
    unsafe {
        Vec::from_raw_parts(
            image.ptr as *mut T,
            (image.width.get() * image.height.get()) as usize * CHANNELS,
            image.data,
        )
    };
}
extern "C" fn clone_slice_into_arc<T: Clone, const CHANNELS: usize>(
    image: &UnsafeGenericImage<T, CHANNELS>,
) -> UnsafeGenericImage<T, CHANNELS> {
    let buffer = unsafe {
        std::slice::from_raw_parts(
            image.ptr,
            image.width.get() as usize * image.height.get() as usize * CHANNELS,
        )
    };
    UnsafeGenericImage::new_arc(Arc::from(buffer), image.width, image.height)
}

impl<TP: std::any::Any, const CHANNELS: usize> Debug for GenericImage<TP, CHANNELS> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenericImage")
            .field("width", &self.0.width)
            .field("height", &self.0.height)
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
            image: &mut UnsafeGenericImage<T, CHANNELS>,
        ) -> *mut T {
            image.ptr as *mut T
        }
        &ImageVtable {
            make_mut,
            drop: clear_vec,
            clone: clone_slice_into_arc,
        }
    };
}

struct ArcFactory;

impl<T: 'static + Clone, const CHANNELS: usize> Factory<T, CHANNELS> for ArcFactory {
    const VTABLE: &'static ImageVtable<T, CHANNELS> = {
        unsafe extern "C" fn make_mut<T: Clone, const CHANNELS: usize>(
            image: &mut UnsafeGenericImage<T, CHANNELS>,
        ) -> *mut T {
            let mut arc = ManuallyDrop::new(unsafe {
                let ptr = std::ptr::slice_from_raw_parts(image.ptr, image.data);
                Arc::<[T]>::from_raw(ptr)
            });

            if let Some(ptr) = Arc::get_mut(&mut arc) {
                ptr.as_mut_ptr()
            } else {
                let mut new_data = Arc::<[T]>::from(&arc[..]);
                ManuallyDrop::into_inner(arc);

                let ptr = Arc::get_mut(&mut new_data).expect("Just created, must be unique");
                let r = ptr.as_mut_ptr();
                image.ptr = Arc::into_raw(new_data).cast::<T>();
                r
            }
        }
        extern "C" fn clear_arc<T: Clone, const CHANNELS: usize>(
            image: &mut UnsafeGenericImage<T, CHANNELS>,
        ) {
            unsafe {
                let ptr = std::ptr::slice_from_raw_parts(image.ptr, image.data);
                Arc::<[T]>::from_raw(ptr);
            }
        }

        extern "C" fn clone_arc<T: Clone, const CHANNELS: usize>(
            image: &UnsafeGenericImage<T, CHANNELS>,
        ) -> UnsafeGenericImage<T, CHANNELS> {
            let arc = ManuallyDrop::new(unsafe {
                let ptr = std::ptr::slice_from_raw_parts(image.ptr, image.data);
                Arc::<[T]>::from_raw(ptr)
            });
            GenericImage::new_arc((*arc).clone(), image.width, image.height).0
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
        Self(UnsafeGenericImage::new_vec(input, width, height))
    }

    pub fn new_arc(input: Arc<[T]>, width: NonZeroU32, height: NonZeroU32) -> Self
    where
        T: Clone,
    {
        Self(UnsafeGenericImage::new_arc(input, width, height))
    }
    pub const fn len(&self) -> usize {
        assert!(self.0.width.get() <= usize::MAX as u32);
        assert!(self.0.height.get() <= usize::MAX as u32);
        self.0.width.get() as usize * self.0.height.get() as usize * CHANNELS
    }

    pub const fn buffer(&self) -> &[T] {
        unsafe { std::slice::from_raw_parts(self.0.ptr, self.len()) }
    }

    pub fn make_mut(&mut self) -> &mut [T] {
        unsafe {
            let ptr = (self.0.vtable.make_mut)(&mut self.0);
            let len = self.len();
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
        Self(UnsafeGenericImage::new_with_vtable(
            buf,
            width,
            height,
            vtable,
            generic_field,
        ))
    }

    pub fn into_vec(self) -> Vec<T>
    where
        T: Clone,
    {
        if self.0.vtable.drop as usize == clear_vec::<T, CHANNELS> as usize {
            let size = self.len();
            let result = unsafe { Vec::from_raw_parts(self.0.ptr as *mut _, size, self.0.data) };
            std::mem::forget(self);
            result
        } else {
            self.buffer().to_vec()
        }
    }
    pub fn dimensions(&self) -> (NonZeroU32, NonZeroU32) {
        (self.0.width, self.0.height)
    }
}

impl<T, const CHANNELS: usize> Drop for UnsafeGenericImage<T, CHANNELS> {
    fn drop(&mut self) {
        if self.ptr as usize != 0 {
            unsafe { (self.vtable.drop)(self) };
        }
    }
}

#[cfg(test)]
mod tests {
    use image::Rgb;

    use super::*;

    #[test]
    fn miri_load_and_save_dynamic_rgb_image() {
        let image = image::ImageBuffer::<Rgb<u8>, _>::from_vec(
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
        let pilatus = GenericImage::<[u8; 3], 1>::new_vec(
            vec_rgb,
            2.try_into().unwrap(),
            2.try_into().unwrap(),
        );
        let dynamic: DynamicImage = pilatus.into();

        let DynamicImage::Rgb8Planar(rgb) = &dynamic else {
            panic!("Buffer contains no rgb-image");
        };

        assert_eq!([0, 3, 6, 9, 1, 4, 7, 10, 2, 5, 8, 11], rgb.buffer());
        let png = dynamic.encode_png().unwrap();
        let image::DynamicImage::ImageRgb8(reloaded) = image::load_from_memory(&png).unwrap()
        else {
            panic!("Buffer contains rgb-image");
        };
        assert_eq!(reloaded, image);
    }

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
        let to_vec = image.into_vec();

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
        let to_vec = image.into_vec();
        let to_vec2 = image2.into_vec();

        assert_ne!(
            to_vec[..].as_ptr(),
            to_vec2[..].as_ptr(),
            "Should reuse the buffer if it was created by vec"
        );
    }

    #[test]
    fn miri_test_into_packed() {
        let size = 2.try_into().unwrap();
        let image = Arc::new(GenericImage::<u8, 3>::new_vec(
            vec![1u8, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3],
            size,
            size,
        ));
        let packed = image.into_packed();
        assert_eq!(
            packed.flat_buffer(),
            vec!(1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3)
        );
    }

    #[test]
    fn miri_test_shared_arc_u16_luma() {
        let arc: Arc<[u16]> = vec![1].into();
        test_entire_vtable(GenericImage::<u16, 1>::new_arc(
            arc,
            NonZeroU32::MIN,
            NonZeroU32::MIN,
        ));
    }
    #[test]
    fn miri_test_exclusive_arc_u16_luma() {
        test_entire_vtable(GenericImage::<u16, 1>::new_arc(
            vec![1].into(),
            NonZeroU32::MIN,
            NonZeroU32::MIN,
        ));
    }
    #[test]
    fn miri_test_vec_u16_luma() {
        test_entire_vtable(GenericImage::<u16, 1>::new_vec(
            vec![1],
            NonZeroU32::MIN,
            NonZeroU32::MIN,
        ));
    }

    fn test_entire_vtable<T: 'static + Default + Eq, const SIZE: usize>(
        mut image: GenericImage<T, SIZE>,
    ) {
        image.make_mut()[0] = T::default();
        let mut clone = image.clone();
        clone.make_mut()[0] = T::default();
        assert_eq!(image, clone);
    }
}
