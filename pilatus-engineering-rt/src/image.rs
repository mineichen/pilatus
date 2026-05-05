mod logo;
mod png;
#[cfg(feature = "axum")]
mod web;

use std::sync::Arc;

#[cfg(feature = "unstable")]
pub use logo::*;
use pilatus_engineering::image::{
    DecodeExtensions, EncodeExtensions, MetaImageDecoder, MetaImageEncoder,
};

pub(super) fn register_services(c: &mut minfac::ServiceCollection) {
    c.with::<minfac::AllRegistered<pilatus_engineering::image::EncodeExtension>>()
        .register_shared(|exts| Arc::new(EncodeExtensions::new(exts)))
        .alias(|exts| MetaImageEncoder::with_extensions(exts));

    c.with::<minfac::AllRegistered<pilatus_engineering::image::DecodeExtension>>()
        .register_shared(|exts| Arc::new(DecodeExtensions::new(exts)))
        .alias(|exts| MetaImageDecoder::with_extensions(exts));

    logo::register_services(c);
    png::register_services(c);
    #[cfg(feature = "axum")]
    web::register_services(c);
}
