use std::{borrow::Cow, collections::HashMap};

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Debug, PartialEq, Eq, Hash)]
pub struct ImageKey(Option<Cow<'static, str>>);

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
#[error("Cannot turn '{value}' into ImageKey: {reason}")]
pub struct IntoImageKeyError {
    value: Cow<'static, str>,
    reason: &'static str,
}

impl TryFrom<&'static str> for ImageKey {
    type Error = IntoImageKeyError;

    fn try_from(value: &'static str) -> Result<Self, Self::Error> {
        Cow::Borrowed(value).try_into()
    }
}

impl TryFrom<Cow<'static, str>> for ImageKey {
    type Error = IntoImageKeyError;

    fn try_from(value: Cow<'static, str>) -> Result<Self, Self::Error> {
        let mut chars = value.chars();
        match chars.next() {
            Some(x) if x.is_alphabetic() => {
                if chars.all(|c| c.is_alphanumeric()) {
                    Ok(ImageKey(Some(value)))
                } else {
                    Err(IntoImageKeyError {
                        value,
                        reason: "Contains chars which are not alphanumeric",
                    })
                }
            }
            Some(_) => Err(IntoImageKeyError {
                value,
                reason: "Must start with alphabetic char",
            }),
            None => Err(IntoImageKeyError {
                value,
                reason: "Mustn't be empty",
            }),
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
