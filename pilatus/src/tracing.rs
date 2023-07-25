use serde::{Deserialize, Deserializer};
use std::{
    collections::HashMap,
    net::SocketAddr,
    ops::Deref,
    path::{Path, PathBuf},
    str::FromStr,
};
use tracing::Level;

use crate::GenericConfig;

#[derive(Debug, Clone, PartialEq)]
pub struct TracingConfig {
    default_level: tracing::Level,
    filters: HashMap<String, tracing::Level>,
    file: Option<TracingFileConfig>,
    console: Option<TracingConsoleConfig>,
}

impl<'a> From<&'a GenericConfig> for TracingConfig {
    fn from(value: &'a GenericConfig) -> Self {
        let p = value
            .get::<TracingConfigPrivate>("tracing")
            .unwrap_or_default();
        Self {
            default_level: p.default_level.0,
            filters: p.filters.into_iter().map(|(k, v)| (k, v.0)).collect(),
            file: p.file,
            console: p.console,
        }
        .instrument_path(value)
    }
}

impl TracingConfig {
    pub fn log_string(&self) -> String {
        std::iter::once(self.default_level.to_string())
            .chain(
                self.filters
                    .iter()
                    .map(|(topic, level)| format!("{topic}={level}")),
            )
            .collect::<Vec<_>>()
            .join(",")
    }

    fn instrument_path(mut self, root: &GenericConfig) -> Self {
        if let Some(file_config) = self.file.as_mut() {
            file_config.path = root.instrument_relative(&file_config.path);
        }
        self
    }

    pub fn directory(&self) -> Option<&Path> {
        self.file.as_ref().map(|x| x.path.deref())
    }

    pub fn file(&self) -> Option<&TracingFileConfig> {
        self.file.as_ref()
    }
    pub fn console(&self) -> Option<&TracingConsoleConfig> {
        self.console.as_ref()
    }
}

#[derive(Debug, Clone)]
struct LevelWrapper(tracing::Level);

impl<'de> Deserialize<'de> for LevelWrapper {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let variant = String::deserialize(deserializer)?;
        let level =
            tracing::Level::from_str(&variant).map_err(<D::Error as serde::de::Error>::custom)?;
        Ok(LevelWrapper(level))
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct TracingConfigPrivate {
    default_level: LevelWrapper,
    filters: HashMap<String, LevelWrapper>,
    file: Option<TracingFileConfig>,
    console: Option<TracingConsoleConfig>,
}

impl Default for TracingConfigPrivate {
    fn default() -> Self {
        Self {
            default_level: LevelWrapper(Level::DEBUG),
            filters: [
                ("hyper", LevelWrapper(Level::INFO)),
                ("request", LevelWrapper(Level::INFO)),
                ("async_zip", LevelWrapper(Level::INFO)),
                ("tower_http", LevelWrapper(Level::INFO)),
                ("mio_serial", LevelWrapper(Level::INFO)),
                ("pilatus::image", LevelWrapper(Level::INFO)),
                ("tungstenite::protocol", LevelWrapper(Level::INFO)),
            ]
            .into_iter()
            .map(|(t, l)| (t.into(), l))
            .collect(),
            file: Some(Default::default()),
            console: Default::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct TracingConsoleConfig {
    pub address: SocketAddr,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct TracingFileConfig {
    pub path: PathBuf,
    pub number_of_files: usize,
}

impl Default for TracingFileConfig {
    fn default() -> Self {
        Self {
            path: "./logs".into(),
            number_of_files: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn deserialize_tracing() {
        let generic = GenericConfig::mock(json!({
            "tracing": {
                "default_level":"trace",
                "filters":{
                    "tokio":"debug",
                    "mio_serial":"info"
                },
            }
        }));

        assert_eq!(
            TracingConfig {
                default_level: Level::TRACE,
                filters: HashMap::from([
                    ("tokio".to_owned(), Level::DEBUG),
                    ("mio_serial".to_owned(), Level::INFO),
                ]),
                file: Some(TracingFileConfig::default()),
                console: None,
            }
            .instrument_path(&generic),
            TracingConfig::from(&generic)
        );
    }
    #[test]
    fn deserialize_tracing_with_file() {
        let generic = GenericConfig::mock(json!({
            "tracing": {
                "default_level":"trace",
                "filters":{
                  "tokio":"debug",
                  "mio_serial":"info"
                },
                "file":{
                  "number_of_files":3,
                  "path":"./mylog"
                }
            }
        }));
        assert_eq!(
            TracingConfig {
                default_level: Level::TRACE,
                filters: HashMap::from([
                    ("tokio".to_owned(), Level::DEBUG),
                    ("mio_serial".to_owned(), Level::INFO),
                ]),
                file: Some(TracingFileConfig {
                    number_of_files: 3,
                    path: "./test_data/mylog".into()
                }),
                console: None,
            },
            TracingConfig::from(&generic)
        );
    }
}
