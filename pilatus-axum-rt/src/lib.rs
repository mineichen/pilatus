mod abort;
mod device;
mod hosted_service;
#[cfg(feature = "engineering")]
mod image;
mod inject;
mod logo;
mod recipe;
mod time;
mod ws;

pub extern "C" fn register(collection: &mut minfac::ServiceCollection) {
    abort::register_services(collection);
    device::register_services(collection);
    hosted_service::register_services(collection);
    #[cfg(feature = "engineering")]
    image::register_services(collection);
    recipe::register_services(collection);
    time::register_services(collection);
    ws::register_services(collection);
    logo::register_services(collection);
}
