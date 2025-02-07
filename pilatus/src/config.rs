use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
};

use config::{builder::DefaultState, ConfigBuilder, ConfigError};
use glob::glob;
use serde::de::DeserializeOwned;
use tracing::{error, info};

/// Devices can recive typed configs for e.g. MagicConstants like timeouts or socket addresses
/// In pilatus it is parsed from all JSON-Files in the root (typically the same folder as the executable)
/// Configuration never changes during runtime. Use settings if this is needed.
#[derive(Clone, Debug, Default)]
pub struct GenericConfig {
    pub root: PathBuf,
    config: config::Config,
}

impl GenericConfig {
    #[cfg(any(test, feature = "unstable"))]
    pub fn mock(config: serde_json::Value) -> Self {
        Self {
            root: "./test_data".into(),
            config: config::Config::builder()
                .add_source(config::Config::try_from(&config).unwrap())
                .build()
                .unwrap(),
        }
    }
    pub fn new(path: impl Into<PathBuf>) -> io::Result<Self> {
        let root = path.into();
        let json_path = root.join("*.json");
        let str = json_path
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid characters"))?;
        let paths: Result<Vec<_>, _> = glob(str)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?
            .filter(|x| {
                x.as_ref()
                    .map(|p| p.file_name() != Some(OsStr::new("settings.json")))
                    .unwrap_or(true)
            })
            .collect();
        let mut paths = paths.map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        paths.sort_unstable();

        let builder = paths
            .into_iter()
            .map(|p| {
                info!("Add settings file: {p:?}");
                config::File::from(p)
            })
            .fold(
                config::Config::builder(),
                ConfigBuilder::<DefaultState>::add_source,
            );

        let config = builder
            .build()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        Ok(Self { config, root })
    }

    pub fn instrument_relative(&self, path: impl Into<PathBuf> + AsRef<Path>) -> PathBuf {
        if path.as_ref().is_relative() {
            self.root.join(path)
        } else {
            path.into()
        }
    }

    pub fn get_or_default<T: DeserializeOwned + Default>(&self, key: &str) -> T {
        match self.config.get::<T>(key) {
            Ok(x) => x,
            Err(ConfigError::Type {
                origin,
                unexpected,
                expected,
                key,
            }) => {
                error!("{key:?} cannot be parsed into {expected}, got {unexpected}, configuration src: {origin:?}. Using default instead");
                T::default()
            }
            Err(_) => Default::default(),
        }
    }

    pub fn try_get<T: DeserializeOwned>(&self, key: &str) -> anyhow::Result<T> {
        Ok(self.config.get::<T>(key)?)
    }

    #[deprecated = "Pattern x.get().unwrap_or_default() was very common, which ignored invalid configurations and silently uses the default instead. Use `get_or_default`, which produces error logs on invalid configs or `try_get` if you really want the errornous configs to be returned as anyhow::Error (try_get is equivalent to get)"]
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> anyhow::Result<T> {
        Ok(self.config.get::<T>(key)?)
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use serde::{Deserialize, Serialize};

    use super::*;

    #[test]
    fn join_two_relative_paths() {
        let config = GenericConfig::new(".").unwrap();
        assert_eq!(
            PathBuf::from("./test"),
            config.instrument_relative(PathBuf::from("./test")),
        )
    }

    #[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    struct Foo {
        bar: String,
        #[serde(default)]
        baz: i32,
    }

    #[test]
    fn get_error_on_missing() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let c = GenericConfig::new(tmp.path())?;
        assert!(c.try_get::<Foo>("foo").is_err());
        Ok(())
    }

    #[test]
    fn get_partial_default() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("develop.json");

        std::fs::write(path, r#"{ "foo":  { "bar": "Test" } }"#)?;
        let c = GenericConfig::new(tmp.path())?;
        assert_eq!(
            c.try_get::<Foo>("foo")?,
            Foo {
                bar: "Test".to_string(),
                baz: 0
            }
        );
        Ok(())
    }
}
