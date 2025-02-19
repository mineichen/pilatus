use std::borrow::Cow;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Debug, PartialEq, Eq, Hash)]
pub struct SpecificImageKey(Cow<'static, str>);

impl SpecificImageKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SpecificImageKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let str = Cow::<'static, str>::deserialize(deserializer)?;
        Ok(str
            .try_into()
            .map_err(<D::Error as serde::de::Error>::custom)?)
    }
}

impl TryFrom<&'static str> for SpecificImageKey {
    type Error = IntoSpecificImageKeyError;

    fn try_from(value: &'static str) -> Result<Self, Self::Error> {
        Cow::Borrowed(value).try_into()
    }
}

impl TryFrom<Cow<'static, str>> for SpecificImageKey {
    type Error = IntoSpecificImageKeyError;

    fn try_from(value: Cow<'static, str>) -> Result<Self, Self::Error> {
        let mut chars = value.chars();
        match chars.next() {
            Some(x) if x.is_alphabetic() => {
                if chars.all(|c| c.is_alphanumeric()) {
                    Ok(Self(value))
                } else {
                    Err(IntoSpecificImageKeyError {
                        value,
                        reason: "Contains chars which are not alphanumeric",
                    })
                }
            }
            Some(_) => Err(IntoSpecificImageKeyError {
                value,
                reason: "Must start with alphabetic char",
            }),
            None => Err(IntoSpecificImageKeyError {
                value,
                reason: "Mustn't be empty",
            }),
        }
    }
}

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
#[error("Cannot turn '{value}' into ImageKey: {reason}")]
pub struct IntoSpecificImageKeyError {
    value: Cow<'static, str>,
    reason: &'static str,
}
