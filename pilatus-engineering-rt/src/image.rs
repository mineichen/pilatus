mod logo;
mod png;
#[cfg(feature = "axum")]
mod web;

#[cfg(feature = "unstable")]
pub use logo::*;

pub(super) fn register_services(c: &mut minfac::ServiceCollection) {
    logo::register_services(c);
    png::register_services(c);
    #[cfg(feature = "axum")]
    web::register_services(c);
}
