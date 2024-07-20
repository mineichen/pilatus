use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::{Arc, RwLock},
};

use minfac::{Registered, ServiceCollection};
use pilatus::{LogoQuery, LogoService};
use tracing::warn;

use super::GenericImage;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<Registered<LogoService>>()
        .register_shared(|s| Arc::new(ImageLogoServiceImpl::new(s)))
        .alias(|s| ImageLogoService::new(s));
}

type Age = u64;

pub trait ImageLogoServiceTrait {
    fn get_logo(&self, query: LogoQuery) -> Arc<GenericImage<4>>;
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

struct ImageLogoServiceImpl {
    cache: RwLock<HashMap<LogoQuery, (Age, Arc<GenericImage<4>>)>>,
    logo_service: LogoService,
}

impl ImageLogoServiceImpl {
    fn new(logo_service: LogoService) -> Self {
        Self {
            cache: Default::default(),
            logo_service,
        }
    }
}

const CACHE_CAPACITY: usize = 10;

impl ImageLogoServiceTrait for ImageLogoServiceImpl {
    /// Panics: GenericImage<4> with packed pixels (rgbargbargbargba, not rrrrggggbbbbaaaa)
    fn get_logo(&self, query: LogoQuery) -> Arc<GenericImage<4>> {
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

        let logo = self.logo_service.get(&query);
        let img = if let Ok(img) = image::load_from_memory(&logo.0[..]) {
            let resized = img.resize(
                query.width.get() as _,
                query.height.get() as _,
                image::imageops::FilterType::Lanczos3,
            );

            let rgba = resized.to_rgba8();
            let isize = rgba.dimensions();
            let (iwidth, iheight) = (
                isize.0.try_into().expect("Input image has width=0"),
                isize.1.try_into().expect("Input image has height=0"),
            );

            GenericImage::<4>::new(rgba.into_vec(), iwidth, iheight)
        } else if let Ok(svg) = resvg::usvg::Tree::from_data(&logo.0, &Default::default()) {
            let x = resvg::usvg::Tree::from(svg);
            let size = x.size();
            let (svg_width, svg_height) = (size.width(), size.height());
            let query_ratio = query.width.get() as f32 / query.height.get() as f32;
            let svg_ratio = svg_width / svg_height;
            let (pixmap_width, pixmap_height, scale): (NonZeroU32, NonZeroU32, f32) =
                if svg_ratio >= query_ratio {
                    (
                        NonZeroU32::from(*query.width),
                        ((query.width.get() as f32 / svg_ratio).round() as u32)
                            .try_into()
                            .unwrap_or(NonZeroU32::MIN),
                        (query.width.get() as f32 / svg_width),
                    )
                } else {
                    (
                        ((query.height.get() as f32 * svg_ratio).round() as u32)
                            .try_into()
                            .unwrap_or(NonZeroU32::MIN),
                        NonZeroU32::from(*query.height),
                        (query.height.get() as f32 / svg_height),
                    )
                };

            let mut pixmap = resvg::tiny_skia::Pixmap::new(pixmap_width.get(), pixmap_height.get())
                .expect("Query width/height are bellow the limit i32/4");

            resvg::render(
                &x,
                resvg::tiny_skia::Transform::from_scale(scale, scale),
                &mut pixmap.as_mut(),
            );

            let out_width = pixmap
                .width()
                .try_into()
                .expect("Generated Image has width=0");
            let out_height = pixmap
                .height()
                .try_into()
                .expect("Generated Image has height=0");

            GenericImage::<4>::new(pixmap.take(), out_width, out_height)
        } else {
            let width = query.width.get();
            let height = query.height.get();

            warn!("The logo is not loadable. Therefore a red surface of the size {width}x{height} was returned");
            GenericImage::new(
                (0..(width * height))
                    .flat_map(|_| [255, 0, 0, 255])
                    .collect(),
                NonZeroU32::from(*query.width),
                NonZeroU32::from(*query.height),
            )
        };

        let image = Arc::new(img);
        lock.insert(query, (next_age, image.clone()));
        image
    }
}

#[cfg(feature = "unstable")]
pub fn create_default_image_logo_service() -> ImageLogoService {
    ImageLogoService::new(Arc::new(ImageLogoServiceImpl::new(
        pilatus_rt::create_default_logo_service(),
    )))
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
    fn get_pixel_logo() {
        let raw_service = Arc::new(StaticLogoService(AtomicU8::new(0), PNG_1X1));
        let image = get_logo(LogoService::new(raw_service.clone()));
        assert_eq!(raw_service.0.load(SeqCst), 1);
        let expect_size = 100.try_into().unwrap();
        assert_eq!((expect_size, expect_size), (image.width, image.height));
    }

    #[test]
    fn get_default_vector_logo() {
        let image = get_logo(pilatus_rt::create_default_logo_service());
        assert_eq!(200, image.width.get());
    }
    #[test]
    fn get_too_wide_vector_logo() {
        let raw_service = Arc::new(StaticLogoService(
            AtomicU8::new(0),
            br#"<?xml version="1.0" encoding="UTF-8" standalone="no"?><svg width="400" height="100" xmlns="http://www.w3.org/2000/svg">
                <rect width="400" height="100" style="fill:rgb(0,0,255)" />
            </svg>"#,
        ));
        let logo = get_logo(LogoService::new(raw_service));
        assert_eq!((logo.width.get(), logo.height.get()), (200, 50));
    }

    #[test]
    fn get_too_heigh_vector_logo() {
        let raw_service = Arc::new(StaticLogoService(
            AtomicU8::new(0),
            br#"<?xml version="1.0" encoding="UTF-8" standalone="no"?><svg width="200" height="200" xmlns="http://www.w3.org/2000/svg">
                <rect width="196" height="196" x="2" y="2" style="fill:rgb(255,0,0)" />
            </svg>"#,
        ));
        let logo = get_logo(LogoService::new(raw_service));
        assert_eq!((logo.width.get(), logo.height.get()), (100, 100));
        let last_row_col = 4 * (100 * 100 - 1);

        assert_eq!(
            &logo.buffer()[last_row_col..(last_row_col + 4)],
            &[0, 0, 0, 0]
        );
        let diag_left = last_row_col - 4 * 101;
        assert_eq!(
            &logo.buffer()[diag_left..(diag_left + 4)],
            &[255, 0, 0, 255]
        );
        assert_eq!(
            &logo.buffer()[(diag_left + 4)..(diag_left + 8)],
            &[0, 0, 0, 0]
        );
    }

    fn get_logo(s: LogoService) -> Arc<GenericImage<4>> {
        let service = ImageLogoServiceImpl::new(s);
        let query = LogoQuery {
            width: 200.try_into().unwrap(),
            height: 100.try_into().unwrap(),
            ..Default::default()
        };
        service.get_logo(query.clone());
        service.get_logo(query)
    }

    #[test]
    fn get_1x1_svg_for_too_wide() {
        let raw_service = Arc::new(StaticLogoService(
            AtomicU8::new(0),
            br#"<?xml version="1.0" encoding="UTF-8" standalone="no"?><svg width="100000" height="100" xmlns="http://www.w3.org/2000/svg">
                <rect width="100000" height="100" style="fill:rgb(0,0,255)" />
            </svg>"#,
        ));
        let service = ImageLogoServiceImpl::new(LogoService::new(raw_service));
        let query = LogoQuery {
            width: 1.try_into().unwrap(),
            height: 1.try_into().unwrap(),
            ..Default::default()
        };
        service.get_logo(query);
    }

    #[test]
    fn get_1x1_svg_for_too_high() {
        let raw_service = Arc::new(StaticLogoService(
            AtomicU8::new(0),
            br#"<?xml version="1.0" encoding="UTF-8" standalone="no"?><svg width="1" height="100000" xmlns="http://www.w3.org/2000/svg">
                <rect width="1" height="100000" style="fill:rgb(0,0,255)" />
            </svg>"#,
        ));
        let service = ImageLogoServiceImpl::new(LogoService::new(raw_service));
        let query = LogoQuery {
            width: 1.try_into().unwrap(),
            height: 1.try_into().unwrap(),
            ..Default::default()
        };
        service.get_logo(query);
    }

    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc,
        0xff, 0x9f, 0xa1, 0x1e, 0x00, 0x07, 0x82, 0x02, 0x7f, 0x3d, 0xc8, 0x48, 0xef, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    struct StaticLogoService(AtomicU8, &'static [u8]);

    impl LogoServiceTrait for StaticLogoService {
        fn get(&self, _query: &LogoQuery) -> pilatus::EncodedImage {
            let before = self.0.load(SeqCst);
            self.0.store(before + 1, SeqCst);
            EncodedImage(Arc::from(self.1))
        }
    }
}
