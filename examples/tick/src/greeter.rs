use serde::{Deserialize, Serialize};

pilatus::unstable_pub!(
    #[derive(Debug, Default, Deserialize, Serialize, impex::Impex)]
    #[serde(deny_unknown_fields)]
    struct Params {
        pub lang: Language,
    }
);

pilatus::unstable_pub!(
    #[derive(Debug, Default, Deserialize, Serialize, Clone)]
    enum Language {
        #[default]
        English,
        German,
    }
);

impl impex::ImpexPrimitive for Language {}
