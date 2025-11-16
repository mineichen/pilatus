use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

pilatus::unstable_pub!(
    #[derive(Debug, Default, Deserialize, Serialize, PartialEq, impex::Impex)]
    #[impex(derive(PartialEq, Clone))]
    #[serde(deny_unknown_fields)]
    struct GreeterParams {
        pub lang: GreeterLanguage,
    }
);

pilatus::unstable_pub!(
    #[derive(Debug, Default, Deserialize, Serialize, Clone, PartialEq)]
    enum GreeterLanguage {
        #[default]
        English,
        German,
    }
);

impl impex::ImpexPrimitive for GreeterLanguage {}

pilatus::unstable_pub!(
    #[derive(Serialize, Deserialize, Default, Debug, PartialEq, impex::Impex)]
    #[impex(derive(PartialEq, Eq, Clone))]
    #[serde(default)]
    struct ManualTickParams {
        pub initial_count: u32,
    }
);

pilatus::unstable_pub!(
    #[derive(Debug, Clone, Deserialize, Serialize, PartialEq, impex::Impex)]
    #[impex(derive(PartialEq, Eq, Clone))]
    #[serde(deny_unknown_fields)]
    struct TimerTickParams {
        pub milli_seconds_per_step: NonZeroU64,
    }
);

impl Default for TimerTickParams {
    fn default() -> Self {
        Self {
            milli_seconds_per_step: const { NonZeroU64::new(100).unwrap() },
        }
    }
}
