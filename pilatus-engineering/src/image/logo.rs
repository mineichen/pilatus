use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use minfac::{Registered, ServiceCollection};
use pilatus::{LogoQuery, LogoService};

use super::GenericImage;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<Registered<LogoService>>()
        .register_shared(|s| Arc::new(ImageLogoServiceImpl::new(s)));
}

type Age = u64;

pub trait ImageLogoServiceTrait {
    fn get_logo_with_height(&self, query: LogoQuery) -> Arc<GenericImage<4>>;
}

pub struct ImageLogoService(Arc<dyn ImageLogoServiceTrait + Send + Sync>);

struct ImageLogoServiceImpl {
    cache: RwLock<HashMap<LogoQuery, (Age, Arc<GenericImage<4>>)>>,
    raw: LogoService,
}

impl ImageLogoServiceImpl {
    fn new(fallback: LogoService) -> Self {
        Self {
            cache: Default::default(),
            raw: fallback,
        }
    }
}

const CACHE_CAPACITY: usize = 10;

impl ImageLogoServiceTrait for ImageLogoServiceImpl {
    /// Panics: GenericImage<4> with packed pixels (rgbargbargbargba, not rrrrggggbbbbaaaa)
    fn get_logo_with_height(&self, query: LogoQuery) -> Arc<GenericImage<4>> {
        let lock = self.cache.read().unwrap();
        if let Some((_, cached)) = lock.get(&query) {
            return cached.clone();
        }
        drop(lock);
        let mut lock = self.cache.write().unwrap();
        if let Some((_, cached)) = lock.get(&query) {
            return cached.clone();
        }

        let (next_age, to_delete) = {
            let mut iter = lock.iter().map(|(query, (age, _))| (*age, query));

            match iter.next() {
                Some(x) => {
                    let (min, max) = iter.fold(
                        (x, x),
                        |((min_age, min_height), (max_age, max_height)), (n_age, n_height)| {
                            (
                                if min_age > n_age {
                                    (n_age, n_height)
                                } else {
                                    (min_age, min_height)
                                },
                                if max_age < n_age {
                                    (n_age, n_height)
                                } else {
                                    (max_age, max_height)
                                },
                            )
                        },
                    );
                    (max.0 + 1, Some(min.1))
                }
                None => (0, None),
            }
        };

        if let (Some(to_delete), true) = (to_delete, lock.len() >= CACHE_CAPACITY) {
            let clone = to_delete.clone();
            lock.remove(&clone).expect("Must exist");
        }

        let logo = self.raw.get(&query);
        let img = image::load_from_memory(&logo.0[..])
            .expect("Fallback logo must be loadable (svg are not yet supported)");
        let resized = img.resize(
            query.width.get() as _,
            query.height.get() as _,
            image::imageops::FilterType::Lanczos3,
        );

        let rgba = resized.to_rgba8();

        let (iwidth, iheight) = rgba.dimensions();
        let image = Arc::new(GenericImage::<4>::new(rgba.into_vec(), iwidth, iheight));
        lock.insert(query, (next_age, image.clone()));
        image
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicU8, Ordering::SeqCst},
        Arc,
    };

    use pilatus::{EncodedImage, LogoQuery, LogoService, LogoServiceTrait};

    use super::*;

    #[test]
    fn get_default_logo() {
        let raw_service = Arc::new(StaticLogoService(AtomicU8::new(0)));
        let service = ImageLogoServiceImpl::new(LogoService::new(raw_service.clone()));
        let query = LogoQuery {
            height: 10.try_into().unwrap(),
            width: 1000.try_into().unwrap(),
            ..Default::default()
        };
        service.get_logo_with_height(query.clone());
        let image = service.get_logo_with_height(query);
        assert_eq!(10, image.width);
        assert_eq!(raw_service.0.load(SeqCst), 1);
    }

    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc,
        0xff, 0x9f, 0xa1, 0x1e, 0x00, 0x07, 0x82, 0x02, 0x7f, 0x3d, 0xc8, 0x48, 0xef, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    struct StaticLogoService(AtomicU8);

    impl LogoServiceTrait for StaticLogoService {
        fn get(&self, _query: &LogoQuery) -> pilatus::EncodedImage {
            let before = self.0.load(SeqCst);
            self.0.store(before + 1, SeqCst);
            EncodedImage(Arc::from(PNG_1X1))
        }
    }
}
