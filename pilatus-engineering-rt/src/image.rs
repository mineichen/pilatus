mod logo;
mod png;
#[cfg(feature = "axum")]
mod web;

use std::sync::Arc;

#[cfg(feature = "unstable")]
pub use logo::*;
use pilatus_engineering::image::{
    MetaDecodeExtensions, MetaEncodeExtensions, MetaImageDecoder, MetaImageEncoder,
};

pub(super) fn register_services(c: &mut minfac::ServiceCollection) {
    c.with::<minfac::AllRegistered<pilatus_engineering::image::MetaEncodeExtension>>()
        .register_shared(|exts| Arc::new(MetaEncodeExtensions::new(exts)))
        .alias(|exts| MetaImageEncoder::with_extensions(exts));

    c.with::<minfac::AllRegistered<pilatus_engineering::image::MetaDecodeExtension>>()
        .register_shared(|exts| Arc::new(MetaDecodeExtensions::new(exts)))
        .alias(|exts| MetaImageDecoder::with_extensions(exts));

    logo::register_services(c);
    png::register_services(c);
    #[cfg(feature = "axum")]
    web::register_services(c);
}
