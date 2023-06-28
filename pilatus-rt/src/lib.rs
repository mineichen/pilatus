mod device;
mod logo;
mod metadata_future;
mod occurance_counter;
mod recipe;
mod runtime;
mod shutdown;
mod tracing;

pub use device::*;
pub(crate) use metadata_future::MetadataFuture;
pub use recipe::TokioFileService;
#[cfg(feature = "test")]
pub use recipe::{testutil::*, RecipeServiceImpl};
pub use runtime::Runtime;

pub extern "C" fn register(collection: &mut minfac::ServiceCollection) {
    device::register_services(collection);
    recipe::register_services(collection);
    shutdown::register_services(collection);
    logo::register_services(collection);
}
