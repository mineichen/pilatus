use serde::{Deserialize, Deserializer};
use std::{
    collections::{hash_map::Entry, HashMap},
    net::SocketAddr,
    ops::Deref,
    path::{Path, PathBuf},
    str::FromStr,
};
use tracing::Level;

use crate::GenericConfig;

// Each tracing can be disabled by setting {file|console|terminal} to None
#[derive(Debug, Clone, PartialEq)]
pub struct TracingConfig {
    default_level: tracing::Level,
    filters: HashMap<String, tracing::Level>,
    file: Option<TracingFileConfig>,
    console: Option<TracingConsoleConfig>,
    terminal: Option<TracingTerminalConfig>,
}

pub struct TracingTopic {
    topic: String,
    level: Level,
}

impl TracingTopic {
    pub fn new(topic: impl Into<String>, level: Level) -> Self {
        Self {
            topic: topic.into(),
            level,
        }
    }
}

impl<'a, T: IntoIterator<Item = TracingTopic>> From<(&'a GenericConfig, T)> for TracingConfig {
    fn from((value, levels): (&'a GenericConfig, T)) -> Self {
        let iter = levels.into_iter();
        let mut filters = HashMap::with_capacity(iter.size_hint().0);
        for l in iter {
            match filters.entry(l.topic) {
                Entry::Occupied(mut e) => {
                    if e.get() < &l.level {
                        e.insert(l.level);
                    }
                }
                Entry::Vacant(v) => {
                    v.insert(l.level);
                }
            }
        }

        let p = value.get_or_default::<TracingConfigPrivate>("tracing");
        filters.extend(p.filters.into_iter().map(|(k, v)| (k, v.0)));
        Self {
            default_level: p.default_level.0,
            filters,
            file: p.file,
            console: p.console,
            terminal: p.terminal,
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
    pub fn terminal(&self) -> Option<&TracingTerminalConfig> {
        self.terminal.as_ref()
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
    terminal: Option<TracingTerminalConfig>,
}

// Enable file and terminal by default
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
            terminal: Some(Default::default()),
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

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct TracingTerminalConfig {
    pub ansi: bool,
}

impl Default for TracingTerminalConfig {
    fn default() -> Self {
        Self {
            ansi: {
                /// Windows cannot display colors in the default cmd, leading to weird looking log messages
                #[cfg(target_os = "windows")]
                {
                    false
                }
                #[cfg(not(target_os = "windows"))]
                {
                    true
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn config_has_precedence_over_suggestion() {
        let generic = GenericConfig::mock(json!({
            "tracing": {
                "filters":{
                    "tokio":"debug"
                },
            }
        }));

        let config = TracingConfig::from((
            &generic,
            [TracingTopic {
                level: Level::TRACE,
                topic: "tokio".into(),
            }],
        ));
        assert_eq!(config.filters.get("tokio"), Some(&Level::DEBUG));
    }

    #[test]
    fn config_takes_most_significant_suggestion() {
        let generic = GenericConfig::mock(json!({}));

        let config = TracingConfig::from((
            &generic,
            [
                TracingTopic {
                    level: Level::DEBUG,
                    topic: "other".into(),
                },
                TracingTopic {
                    level: Level::TRACE,
                    topic: "other".into(),
                },
                TracingTopic {
                    level: Level::DEBUG,
                    topic: "other".into(),
                },
            ],
        ));
        assert_eq!(config.filters.get("other"), Some(&Level::TRACE));
    }

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
                file: Some(Default::default()),
                console: None,
                terminal: Some(Default::default())
            }
            .instrument_path(&generic),
            TracingConfig::from((&generic, []))
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
                terminal: Some(Default::default())
            },
            TracingConfig::from((&generic, []))
        );
    }
}
