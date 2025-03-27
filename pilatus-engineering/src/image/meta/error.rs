use std::fmt::Debug;

use super::{ImageKey, SpecificImageKey};

#[derive(Debug, thiserror::Error)]
#[error("Unknown image key: {search_key:?}. Available are {available_keys:?}")]
pub struct UnknownKeyError<'a, T: Debug> {
    pub main_image: &'a T,
    pub search_key: &'a ImageKey,
    pub available_keys: std::collections::hash_map::Keys<'a, SpecificImageKey, T>,
}

impl<T: Debug + Clone + Send + Sync> From<UnknownKeyError<'_, T>> for (T, anyhow::Error) {
    fn from(val: UnknownKeyError<'_, T>) -> Self {
        let image = val.main_image.clone();
        let error = anyhow::anyhow!("{val:?}");
        (image, error)
    }
}
