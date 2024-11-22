use std::{borrow::Cow, collections::HashMap};

use serde::{Deserialize, Serialize};

use super::{IntoSpecificImageKeyError, SpecificImageKey};

#[derive(Clone, Serialize, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct ImageKey(Option<SpecificImageKey>);

impl TryFrom<&'static str> for ImageKey {
    type Error = IntoSpecificImageKeyError;

    fn try_from(value: &'static str) -> Result<Self, Self::Error> {
        Cow::Borrowed(value).try_into()
    }
}

impl TryFrom<Cow<'static, str>> for ImageKey {
    type Error = IntoSpecificImageKeyError;

    fn try_from(value: Cow<'static, str>) -> Result<Self, Self::Error> {
        Ok(Self(Some(value.try_into()?)))
    }
}

impl ImageKey {
    pub const fn unspecified() -> Self {
        Self(None)
    }
    pub(in super::super) fn by_name_or<'a, T>(
        &self,
        collection: &'a HashMap<SpecificImageKey, T>,
        default: &'a T,
    ) -> Option<&'a T> {
        if let Some(x) = self.0.as_ref() {
            collection.get(x)
        } else {
            Some(default)
        }
    }

    pub(in super::super) fn insert_or<T>(
        self,
        mut value: T,
        col: &mut HashMap<SpecificImageKey, T>,
        fallback: &mut T,
    ) -> Option<T> {
        match self.0 {
            None => {
                std::mem::swap(&mut value, fallback);
                Some(value)
            }
            Some(key) => col.insert(key.into(), value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_null() {
        let key: ImageKey = serde_json::from_str("null").unwrap();
        assert_eq!(ImageKey::unspecified(), key);
    }
}
