mod image;

pub extern "C" fn register(collection: &mut minfac::ServiceCollection) {
    image::register_services(collection);
}
