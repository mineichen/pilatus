use std::{borrow::Cow, collections::HashMap};

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Debug, PartialEq, Eq, Hash)]
pub struct ImageKey(Option<Cow<'static, str>>);

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
#[error("Cannot turn '{0}' into ImageKey")]
pub struct IntoImageKeyError(Cow<'static, str>);

impl TryFrom<&'static str> for ImageKey {
    type Error = IntoImageKeyError;

    fn try_from(value: &'static str) -> Result<Self, Self::Error> {
        Cow::Borrowed(value).try_into()
    }
}

impl TryFrom<Cow<'static, str>> for ImageKey {
    type Error = IntoImageKeyError;

    fn try_from(value: Cow<'static, str>) -> Result<Self, Self::Error> {
        if value.chars().all(|c| c.is_alphabetic()) && !value.is_empty() {
            Ok(ImageKey(Some(value)))
        } else {
            Err(IntoImageKeyError(value))
        }
    }
}

impl<'de> Deserialize<'de> for ImageKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match Option::<Cow<'static, str>>::deserialize(deserializer)? {
            Some(x) => Ok(x
                .try_into()
                .map_err(<D::Error as serde::de::Error>::custom)?),
            None => Ok(ImageKey(None)),
        }
    }
}

impl ImageKey {
    pub const fn unspecified() -> Self {
        Self(None)
    }
    pub(super) fn by_name_or<'a, T>(
        &self,
        collection: &'a ImageCollection<T>,
        default: &'a T,
    ) -> Option<&'a T> {
        if let Some(x) = self.0.as_ref() {
            collection.0.get(x.as_ref())
        } else {
            Some(default)
        }
    }

    pub(super) fn insert_or<T>(
        self,
        mut value: T,
        col: &mut ImageCollection<T>,
        fallback: &mut T,
    ) -> Option<T> {
        match self.0 {
            None => {
                std::mem::swap(&mut value, fallback);
                Some(value)
            }
            Some(key) => col.0.insert(key.into(), value),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ImageCollection<T>(HashMap<String, T>);

impl<T> Default for ImageCollection<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}
