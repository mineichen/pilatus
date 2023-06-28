use std::num::NonZeroU16;

use serde::Deserialize;

#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
pub struct LogoDimension(NonZeroU16);

#[derive(thiserror::Error, Debug)]
#[error("Invalid logo height: {0}. Must be 0<height<=10000")]
pub struct InvalidLogoHeight(u16);

impl TryFrom<u16> for LogoDimension {
    type Error = InvalidLogoHeight;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        let non_zero = NonZeroU16::try_from(value).map_err(|_| InvalidLogoHeight(0))?;
        if non_zero.get() > 10000 {
            return Err(InvalidLogoHeight(value));
        }
        Ok(LogoDimension(non_zero))
    }
}

impl Default for LogoDimension {
    fn default() -> Self {
        Self(300.try_into().unwrap())
    }
}

impl std::ops::Deref for LogoDimension {
    type Target = NonZeroU16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for LogoDimension {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let dim = u16::deserialize(deserializer)?;
        dim.try_into()
            .map_err(<D::Error as serde::de::Error>::custom)
    }
}
