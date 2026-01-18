use std::path::PathBuf;

use pilatus::{Name, device::DeviceId, unstable_pub};
use serde::{Deserialize, Serialize};

mod permanent_recording;
#[cfg(feature = "unstable")]
pub use permanent_recording::*;

unstable_pub!(
    #[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq)]
    #[cfg_attr(feature = "impex", derive(impex::Impex))]
    #[cfg_attr(feature = "impex", impex(derive(PartialEq, Clone)))]
    #[serde(deny_unknown_fields, default)]
    struct Params {
        pub mode: EmulationMode,
        pub file: FileParams,
        pub streaming: StreamingParams,
        pub permanent_recording: Option<permanent_recording::PermanentRecordingConfig>,
    }
);

unstable_pub!(
    #[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq, Eq)]
    #[cfg_attr(feature = "impex", derive(impex::Impex))]
    #[cfg_attr(feature = "impex", impex(derive(PartialEq, Clone)))]
    #[serde(deny_unknown_fields)]
    enum EmulationMode {
        #[default]
        File,
        Streaming,
    }
);

unstable_pub!(
    #[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
    #[cfg_attr(feature = "impex", derive(impex::Impex))]
    #[cfg_attr(feature = "impex", impex(derive(PartialEq, Clone)))]
    #[serde(deny_unknown_fields, default)]
    struct FileParams {
        pub active: ActiveRecipe,
        pub auto_restart: bool,
        pub file_ending: String,
        pub interval: u64,
    }
);

impl FileParams {
    pub fn requires_collection_reload(&self, new_params: &Self) -> bool {
        self.active != new_params.active || self.file_ending != new_params.file_ending
    }
}

unstable_pub!(
    #[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq)]
    #[cfg_attr(feature = "impex", derive(impex::Impex))]
    #[cfg_attr(feature = "impex", impex(derive(PartialEq, Clone)))]
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
    #[derive(Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    #[cfg_attr(feature = "impex", derive(impex::Impex))]
    #[cfg_attr(feature = "impex", impex(derive(PartialEq, Clone)))]
    enum ActiveRecipe {
        #[default]
        Undefined,
        Named(Name),
        External(PathBuf),
    }
);

#[cfg(test)]
#[cfg(feature = "impex")]
mod tests {
    use super::*;

    #[test]
    fn parse_params() {
        let params = Params::default();
        let serialized = serde_json::to_string(&params).unwrap();
        println!("serialized = {}", serialized);
        let _deserialized: ParamsImpex = serde_json::from_str(&serialized).unwrap();
    }
}
