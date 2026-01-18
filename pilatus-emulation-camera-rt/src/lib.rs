mod delete_collection;
mod device;
mod get_image;
mod list_collections;
mod pause;
mod permanent_recording;
mod publish_frame;
mod record;
mod subscribe;
mod upload_image;

pub use device::*;
use minfac::ServiceCollection;

pub extern "C" fn register(c: &mut ServiceCollection) {
    record::register_services(c);
    device::register_services(c);
    pause::register_services(c);
    list_collections::register_services(c);
    upload_image::register_services(c);
    delete_collection::register_services(c);
}
