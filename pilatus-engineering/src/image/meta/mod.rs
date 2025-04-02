use std::collections::HashMap;
use std::fmt::Debug;

use serde::{Deserialize, Serialize};

use super::{StableHash, UnsupportedImageError};

mod any_multimap;
mod error;
mod keys;

pub use any_multimap::*;
pub use error::*;
pub use keys::*;

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ImageWithMeta<T> {
    pub image: T,
    pub meta: ImageMeta,
    pub other: HashMap<SpecificImageKey, T>,
    pub extensions: AnyMultiMap,
}

impl<T> ImageWithMeta<T> {
    pub fn with_meta(image: T, meta: ImageMeta) -> Self {
        Self {
            image,
            meta,
            other: Default::default(),
            extensions: Default::default(),
        }
    }

    pub fn with_hash(image: T, hash: Option<StableHash>) -> Self {
        Self {
            image,
            meta: ImageMeta { hash },
            other: Default::default(),
            extensions: Default::default(),
        }
    }

    #[deprecated = "Image with Meta got plugins which should also be forwarded. Dont destruct this object, but instead modify it"]
    pub fn with_meta_and_others(
        image: T,
        meta: ImageMeta,
        other: HashMap<SpecificImageKey, T>,
    ) -> Self {
        Self {
            image,
            meta,
            other,
            extensions: Default::default(),
        }
    }

    /// Ok if the key is found or unspecified, Err if a key was specified but not found (returning the main image then)
    /// ```
    /// use pilatus_engineering::image::{ImageWithMeta, ImageKey};
    ///
    /// let mut image = ImageWithMeta::with_hash((2,2), None);
    /// let bar_key: ImageKey = "bar".try_into().unwrap();
    /// image.insert(bar_key.clone(), (4,4));
    /// assert_eq!(&(2,2), image.by_key(&ImageKey::unspecified()).unwrap());
    /// assert_eq!(&(4,4), image.by_key(&bar_key).unwrap());
    ///
    /// image.image = (5, 5);
    /// assert_eq!(&(5,5), image.by_key(&ImageKey::unspecified()).unwrap());
    /// assert_eq!(Some((5,5)), image.insert(ImageKey::unspecified(), (6,6)));
    /// assert_eq!(&(6,6), image.by_key(&ImageKey::unspecified()).unwrap());
    /// ```
    pub fn by_key<'a: 'b, 'b>(
        &'a self,
        search_key: &'b ImageKey,
    ) -> Result<&'a T, UnknownKeyError<'b, T>>
    where
        T: Debug,
    {
        search_key
            .by_key_or(&self.other, &self.image)
            .ok_or_else(|| UnknownKeyError {
                main_image: &self.image,
                search_key,
                available_keys: self.other.keys(),
            })
    }

    // Returns The old value
    pub fn insert(&mut self, key: ImageKey, value: T) -> Option<T> {
        key.insert_or(value, &mut self.other, &mut self.image)
    }
}

impl ImageWithMeta<super::DynamicImage> {
    pub fn with_format_by_key<'a: 'b, 'b, T>(
        &'a self,
        search_key: &'b ImageKey,
    ) -> Result<&'a T, ExtractWithFormatError<'b>>
    where
        T: Debug,
        for<'c> &'c T: TryFrom<
            &'c super::DynamicImage,
            Error = UnsupportedImageError<&'c super::DynamicImage>,
        >,
    {
        let x = self.by_key(search_key)?;
        Ok(x.try_into().unwrap())
    }
}
#[derive(Debug, thiserror::Error)]
pub enum ExtractWithFormatError<'a> {
    #[error("{0:?}")]
    UnknownKey(UnknownKeyError<'a, super::DynamicImage>),
    #[error("{0:?}")]
    Unsupported(UnsupportedImageError<&'a super::DynamicImage>),
}

impl<'a> From<UnknownKeyError<'a, super::DynamicImage>> for ExtractWithFormatError<'a> {
    fn from(value: UnknownKeyError<'a, super::DynamicImage>) -> Self {
        Self::UnknownKey(value)
    }
}

/// Allow convenience, as it is not relevant for production workloads
impl<'a> From<ExtractWithFormatError<'a>> for (super::DynamicImage, anyhow::Error) {
    fn from(val: ExtractWithFormatError<'a>) -> Self {
        match val {
            ExtractWithFormatError::UnknownKey(x) => x.into(),
            ExtractWithFormatError::Unsupported(x) => {
                let img = x.0.clone();
                let unsup = UnsupportedImageError((), x.1);
                (img, anyhow::Error::from(unsup))
            }
        }
    }
}

impl<T> IntoIterator for ImageWithMeta<T> {
    type Item = (ImageKey, T);
    type IntoIter = std::iter::Chain<
        std::iter::Once<(ImageKey, T)>,
        std::iter::Map<
            std::collections::hash_map::IntoIter<SpecificImageKey, T>,
            fn((SpecificImageKey, T)) -> (ImageKey, T),
        >,
    >;

    fn into_iter(self) -> Self::IntoIter {
        std::iter::once((ImageKey::unspecified(), self.image)).chain(
            self.other
                .into_iter()
                .map((|(key, i)| (key.into(), i)) as fn((SpecificImageKey, T)) -> (ImageKey, T)),
        )
    }
}

impl<T> ImageWithMeta<T> {
    pub fn iter(&self) -> impl Iterator<Item = (ImageKey, &T)> {
        std::iter::once((ImageKey::unspecified(), &self.image))
            .chain(self.other.iter().map(|(key, i)| (key.clone().into(), i)))
    }
}

impl<T> std::ops::Deref for ImageWithMeta<T> {
    type Target = ImageMeta;

    fn deref(&self) -> &Self::Target {
        &self.meta
    }
}

impl<T> std::ops::DerefMut for ImageWithMeta<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.meta
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ImageMeta {
    pub hash: Option<StableHash>,
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use crate::image::{DynamicImage, GenericImage};

    use super::*;

    #[test]
    fn miri_insert_and_extract_image_luma16() {
        let image = GenericImage::<u16, 1>::new_vec(vec![1], NonZeroU32::MIN, NonZeroU32::MIN);
        let dynamic: DynamicImage = image.into();
        let meta = ImageWithMeta::with_meta(dynamic, ImageMeta { hash: None });
        let back: &GenericImage<u16, 1> =
            meta.with_format_by_key(&ImageKey::unspecified()).unwrap();
        assert_eq!((NonZeroU32::MIN, NonZeroU32::MIN), back.dimensions());
    }
}
