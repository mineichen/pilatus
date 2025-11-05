macro_rules! wrapped_name {
    ($name:ident) => {
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub struct $name(std::sync::Arc<crate::Name>);

        impl $name {
            pub fn suggest_unique(&self) -> impl Iterator<Item = Self> {
                self.0
                    .suggest_unique()
                    .map(|x| Self(std::sync::Arc::new(x)))
            }
        }

        impl std::ops::Deref for $name {
            type Target = str;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl From<$name> for std::sync::Arc<crate::Name> {
            fn from(other: $name) -> Self {
                other.0
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
                crate::Name::deserialize(deserializer).map(|x| Self(std::sync::Arc::new(x)))
            }
        }

        impl std::str::FromStr for $name {
            type Err = <crate::Name as std::str::FromStr>::Err;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                crate::Name::from_str(s).map(|x| Self(std::sync::Arc::new(x)))
            }
        }
    };
}
pub(crate) use wrapped_name;
