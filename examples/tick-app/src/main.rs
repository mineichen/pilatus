use minfac::ServiceCollection;
use pilatus::Name;
use pilatus_rt::Runtime;

fn main() {
    Runtime::default()
        .register(pilatus_axum_rt::register)
        .register(pilatus_tick_rt::register)
        .register(register)
        .run();
}

extern "C" fn register(c: &mut ServiceCollection) {
    c.register(|| {
        // Defines the default actor configuration
        pilatus::InitRecipeListener::new(move |r| {
            r.add_device(
                pilatus_tick_rt::create_default_timer_tick_device_config()
                    .with_name(Name::new("Timer").unwrap()),
            );
            r.add_device(
                pilatus_tick_rt::create_default_manual_tick_device_config()
                    .with_name(Name::new("Manual").unwrap()),
            );
            r.add_device(
                pilatus_tick_rt::create_default_greeter_device_config()
                    .with_name(Name::new("Greeter").unwrap()),
            );
        })
    });
}
