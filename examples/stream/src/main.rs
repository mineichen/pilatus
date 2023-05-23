use minfac::ServiceCollection;
use pilatus::Name;
use pilatus_rt::Runtime;

mod producer;
mod web;

fn main() {
    Runtime::default()
        .register(pilatus_axum_rt::register)
        .register(register)
        .run();
}

extern "C" fn register(c: &mut ServiceCollection) {
    producer::register_services(c);
    web::register_services(c);
    c.register(|| {
        // Defines the default actor configuration
        pilatus::InitRecipeListener::new(move |r| {
            r.add_device(
                producer::create_default_device_config().with_name(Name::new("Coinbase").unwrap()),
            );
        })
    });
}
