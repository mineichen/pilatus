mod abort;
mod device;
mod frontend_config;
mod hosted_service;
mod inject;
mod logo;
mod logs;
mod recipe;
mod time;
mod ws;
mod zip_writer_wrapper;

pub extern "C" fn register(collection: &mut minfac::ServiceCollection) {
    abort::register_services(collection);
    device::register_services(collection);
    hosted_service::register_services(collection);
    recipe::register_services(collection);
    time::register_services(collection);
    ws::register_services(collection);
    logo::register_services(collection);
    logs::register_services(collection);
    frontend_config::register_services(collection);
}
