use std::path::PathBuf;
use std::str::FromStr;

use pilatus::Name;
use pilatus::device::DeviceId;
use serde::{Deserialize, Serialize};

#[cfg(feature = "rt")]
mod device;
#[cfg(feature = "rt")]
mod list_collections;
#[cfg(feature = "rt")]
mod pause;
mod permanent_recording;
#[cfg(feature = "rt")]
mod publish_frame;
#[cfg(feature = "rt")]
mod record;
#[cfg(feature = "rt")]
mod subscribe;

#[cfg(feature = "rt")]
pub use device::*;

#[macro_export]
macro_rules! unstable_pub {
    ($(#[$attr:meta])* struct $name:ident $($rest:tt)*) => {
        $(#[$attr])*
        #[cfg(feature = "unstable")]
        pub struct $name $($rest)*
        $(#[$attr])*
        #[cfg(not(feature = "unstable"))]
        pub(crate) struct $name $($rest)*
    };
    ($(#[$attr:meta])* enum $name:ident $($rest:tt)*) => {
        $(#[$attr])*
        #[cfg(feature = "unstable")]
        pub enum $name $($rest)*
        $(#[$attr])*
        #[cfg(not(feature = "unstable"))]
        pub(crate) enum $name $($rest)*
    };
}

unstable_pub!(
    #[derive(Debug, Deserialize, Serialize, Clone, Default)]
    #[serde(deny_unknown_fields, default)]
    struct Params {
        pub mode: EmulationMode,
        pub file: FileParams,
        pub streaming: StreamingParams,
        permanent_recording: Option<permanent_recording::PermanentRecordingConfig>,
    }
);

unstable_pub!(
    #[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    enum EmulationMode {
        #[default]
        File,
        Streaming,
    }
);

unstable_pub!(
    #[derive(Debug, Deserialize, Serialize, Clone)]
    #[serde(deny_unknown_fields, default)]
    struct FileParams {
        active: ActiveRecipe,
        auto_restart: bool,
        file_ending: String,
        interval: u64,
    }
);

unstable_pub!(
    #[derive(Debug, Deserialize, Serialize, Clone, Default)]
    #[serde(deny_unknown_fields, default)]
    struct StreamingParams {
        pub source_device_id: Option<DeviceId>,
    }
);

impl Default for FileParams {
    fn default() -> Self {
        Self {
            active: Default::default(),
            auto_restart: true,
            file_ending: "png".into(),
            interval: 500,
        }
    }
}

unstable_pub!(
    /// Strings which are valid Names, so don't contain any slashes/backward-slashes, are interpreted as recorded collections. Otherwise it's assumed to be a path. Use ./foo if you want a folder located in $PWD
    #[derive(Default, Debug, Clone, PartialEq)]
    enum ActiveRecipe {
        #[default]
        Undefined,
        Named(Name),
        External(PathBuf),
    }
);
impl Serialize for ActiveRecipe {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ActiveRecipe::Undefined => Option::<()>::None.serialize(serializer),
            ActiveRecipe::Named(name_wrapper) => name_wrapper.as_str().serialize(serializer),
            ActiveRecipe::External(path_buf) => path_buf.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ActiveRecipe {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match Option::<String>::deserialize(deserializer)? {
            Some(x) => match Name::from_str(&x) {
                Ok(x) => Ok(Self::Named(x)),
                Err(_) => Ok(Self::External(PathBuf::from(x))),
            },
            None => Ok(Self::Undefined),
        }
    }
}
