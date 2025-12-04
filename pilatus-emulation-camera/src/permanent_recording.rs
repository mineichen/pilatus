use pilatus::{Name, device::DeviceId, unstable_pub};
use serde::{Deserialize, Serialize};

unstable_pub!(
    #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
    pub(crate) struct PermanentRecordingConfig {
        pub collection_name: Name,
        pub source_id: DeviceId,
    }
);

#[cfg(feature = "impex")]
impl impex::ImpexPrimitive for PermanentRecordingConfig {}

impl PermanentRecordingConfig {
    pub fn collection_path(&self) -> &std::path::Path {
        std::path::Path::new(self.collection_name.as_str())
    }
}
