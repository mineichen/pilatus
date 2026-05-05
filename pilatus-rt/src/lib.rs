#![doc = include_str!("../README.md")]

mod device;
mod logo;
mod metadata_future;
mod recipe;
mod runtime;
mod shutdown;
mod tracing;

use std::io;
use std::path::Path;

pub use device::*;
#[cfg(feature = "unstable")]
pub use logo::create_default_logo_service;
pub use recipe::TokioFileService;
#[cfg(feature = "unstable")]
pub use recipe::*;
pub use tracing::TracingState;

pub use runtime::Runtime;
// Helpers for integration tests
pub use runtime::{TempConfiguredRuntime, TempRuntime};

pub extern "C" fn register(collection: &mut minfac::ServiceCollection) {
    device::register_services(collection);
    recipe::register_services(collection);
    shutdown::register_services(collection);
    logo::register_services(collection);
}

fn with_file_context(file: &Path) -> impl Fn(io::Error) -> io::Error + '_ {
    |e| match e.kind() {
        io::ErrorKind::NotFound => {
            std::io::Error::new(io::ErrorKind::NotFound, file.display().to_string())
        }
        _ => e,
    }
}
