use minfac::ServiceCollection;
use pilatus::{Name, device::ActorMessage};
use pilatus_rt::Runtime;

mod greeter;
mod manual_tick;
mod timer_tick;

fn main() {
    Runtime::default()
        .register(pilatus_axum_rt::register)
        .register(register)
        .run();
}

extern "C" fn register(c: &mut ServiceCollection) {
    greeter::register_services(c);
    manual_tick::register_services(c);
    timer_tick::register_services(c);

    c.register(|| {
        // Defines the default actor configuration
        pilatus::InitRecipeListener::new(move |r| {
            r.add_device(
                timer_tick::create_default_device_config().with_name(Name::new("Timer").unwrap()),
            );
            r.add_device(
                manual_tick::create_default_device_config().with_name(Name::new("Manual").unwrap()),
            );
            r.add_device(
                greeter::create_default_device_config().with_name(Name::new("Greeter").unwrap()),
            );
        })
    });
}

struct GetTickMessage;
impl ActorMessage for GetTickMessage {
    type Output = u32;
    type Error = std::convert::Infallible;
}
