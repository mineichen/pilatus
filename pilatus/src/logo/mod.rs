use std::{collections::HashMap, sync::Arc};

use serde::Deserialize;
use tracing::log::warn;

use crate::Name;

mod dimension;
pub use dimension::*;

#[non_exhaustive]
pub struct FallbackLogo {
    pub main: &'static [u8],
    pub themes: &'static [(&'static str, &'static [u8])],
}

#[derive(Default, Deserialize, Hash, Eq, PartialEq, Clone, Debug)]
#[serde(default)]
pub struct LogoQuery {
    pub theme: Option<Name>,
    pub height: LogoDimension,
    pub width: LogoDimension,
}

impl FallbackLogo {
    pub fn new(main: &'static [u8]) -> Self {
        Self { main, themes: &[] }
    }
    pub fn with_themes(
        main: &'static [u8],
        themes: &'static [(&'static str, &'static [u8])],
    ) -> Self {
        Self { main, themes }
    }
}

impl From<FallbackLogo> for (EncodedImage, HashMap<Name, EncodedImage>) {
    fn from(value: FallbackLogo) -> Self {
        (
            EncodedImage(Arc::from(value.main)),
            value
                .themes
                .iter()
                .filter_map(|(name_raw, data)| match Name::new(*name_raw) {
                    Ok(name) => Some((name, EncodedImage(Arc::from(*data)))),
                    Err(_) => {
                        warn!("Skip invalid name for logo: {name_raw}");
                        None
                    }
                })
                .collect(),
        )
    }
}

#[derive(Clone)]
pub struct EncodedImage(pub Arc<[u8]>);

#[derive(Clone)]
pub struct LogoService(Arc<dyn LogoServiceTrait + Send + Sync>);

impl LogoService {
    pub fn new(inner: Arc<dyn LogoServiceTrait + Send + Sync>) -> Self {
        Self(inner)
    }
}

impl std::ops::Deref for LogoService {
    type Target = Arc<dyn LogoServiceTrait + Send + Sync>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub trait LogoServiceTrait {
    fn get(&self, query: &LogoQuery) -> EncodedImage;
}
