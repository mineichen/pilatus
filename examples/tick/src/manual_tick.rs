use serde::{Deserialize, Serialize};

pilatus::unstable_pub!(
    #[derive(Serialize, Deserialize, Default, Debug, impex::Impex)]
    #[serde(default)]
    struct Params {
        pub initial_count: u32,
    }
);
