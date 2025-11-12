use minfac::ServiceCollection;
use pilatus::device::ActorMessage;

mod greeter;
mod manual_tick;
mod timer_tick;

pub extern "C" fn register(c: &mut ServiceCollection) {
    greeter::register_services(c);
    manual_tick::register_services(c);
    timer_tick::register_services(c);
}

pub use greeter::create_default_device_config as create_default_greeter_device_config;
pub use manual_tick::create_default_device_config as create_default_manual_tick_device_config;
pub use timer_tick::create_default_device_config as create_default_timer_tick_device_config;

struct GetTickMessage;
impl ActorMessage for GetTickMessage {
    type Output = u32;
    type Error = std::convert::Infallible;
}
