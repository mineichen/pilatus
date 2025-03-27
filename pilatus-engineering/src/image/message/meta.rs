use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{ImageKey, SpecificImageKey, StableHash};

#[non_exhaustive]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ImageWithMeta<T> {
    pub image: T,
    pub meta: ImageMeta,
    pub other: HashMap<SpecificImageKey, T>,
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
