mod device;
mod logo;
mod metadata_future;
mod recipe;
mod runtime;
mod shutdown;
mod tracing;

pub use device::*;
#[cfg(feature = "unstable")]
pub use logo::create_default_logo_service;
pub use recipe::TokioFileService;
#[cfg(feature = "unstable")]
pub use recipe::{unstable::*, RecipeServiceFassade};
pub use runtime::Runtime;

pub extern "C" fn register(collection: &mut minfac::ServiceCollection) {
    device::register_services(collection);
    recipe::register_services(collection);
    shutdown::register_services(collection);
    logo::register_services(collection);
}
