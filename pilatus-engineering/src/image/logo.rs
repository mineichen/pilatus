use std::sync::Arc;

use pilatus::LogoQuery;

use imbuf::Image;

pub trait ImageLogoServiceTrait {
    fn get_logo(&self, query: LogoQuery) -> Image<[u8; 4], 1>;
}

#[derive(Clone)]
pub struct ImageLogoService(Arc<dyn ImageLogoServiceTrait + Send + Sync>);

impl ImageLogoService {
    pub fn new(inner: Arc<dyn ImageLogoServiceTrait + Send + Sync>) -> Self {
        Self(inner)
    }
}

impl std::ops::Deref for ImageLogoService {
    type Target = dyn ImageLogoServiceTrait + Send + Sync;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}
