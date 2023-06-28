pub mod image;
mod spatial;

pub use spatial::*;

#[cfg(feature = "image-algorithm")]
pub extern "C" fn register(collection: &mut minfac::ServiceCollection) {
    image::register_services(collection);
}
