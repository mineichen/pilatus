use minfac::ServiceCollection;

mod emulation;

pub extern "C" fn register(c: &mut ServiceCollection) {
    emulation::register_services(c);
}
