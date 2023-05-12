macro_rules! wrapped_uuid {
    ($name:ident) => {
        #[derive(
            Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, sealedstruct::IntoSealed,
        )]
        pub struct $name(uuid::Uuid);

        impl $name {
            pub fn new_v4() -> Self {
                Self(uuid::Uuid::new_v4())
            }

            pub fn nil() -> Self {
                Self(uuid::Uuid::nil())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Display::fmt(&self.0, f)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                self.0.serialize(serializer)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                uuid::Uuid::deserialize(deserializer).map(Self)
            }
        }

        impl std::str::FromStr for $name {
            type Err = <uuid::Uuid as std::str::FromStr>::Err;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                uuid::Uuid::from_str(s).map(Self)
            }
        }
    };
}
pub(crate) use wrapped_uuid;
