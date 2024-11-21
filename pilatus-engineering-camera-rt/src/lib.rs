use minfac::ServiceCollection;

mod emulation;

pub extern "C" fn register(c: &mut ServiceCollection) {
    emulation::register_services(c);
}

pub use emulation::create_default_device_config as create_default_emulation_device_config;
