mod image;

#[cfg(feature = "unstable")]
pub use image::*;

pub extern "C" fn register(collection: &mut minfac::ServiceCollection) {
    image::register_services(collection);
}
