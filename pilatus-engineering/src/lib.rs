pub mod image;
mod spatial;

pub use spatial::*;

#[cfg(feature = "image-algorithm")]
pub extern "C" fn register(collection: &mut minfac::ServiceCollection) {
    #[cfg(feature = "image-algorithm")]
    image::register_services(collection);
}
