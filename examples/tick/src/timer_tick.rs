use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};

pilatus::unstable_pub!(
    #[derive(Debug, Clone, Deserialize, Serialize, impex::Impex)]
    #[serde(deny_unknown_fields)]
    struct Params {
        pub milli_seconds_per_step: NonZeroU64,
    }
);

impl Default for Params {
    fn default() -> Self {
        Self {
            milli_seconds_per_step: const { NonZeroU64::new(100).unwrap() },
        }
    }
}
