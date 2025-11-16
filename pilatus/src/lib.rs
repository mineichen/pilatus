mod config;
pub mod device;
mod entry_io;
#[cfg(feature = "tokio")]
mod file;
#[cfg(all(feature = "tokio", feature = "minfac"))]
mod hosted_service;
mod logo;
mod name;
mod recipe;
mod relative;
mod settings;
mod shutdown;
#[cfg(feature = "subscribe")]
mod subscribe;
mod sync;
mod tracing;
mod unstable;
mod uuid_wrapper;

pub use crate::config::GenericConfig;
pub use crate::tracing::*;
pub use entry_io::*;
#[cfg(feature = "tokio")]
pub use file::*;
#[cfg(all(feature = "tokio", feature = "minfac"))]
pub use hosted_service::HostedService;
pub use logo::*;
pub use name::*;
pub use recipe::*;
pub use relative::*;
pub use settings::Settings;
pub use shutdown::*;
#[cfg(feature = "subscribe")]
pub use subscribe::*;
pub use sync::*;

#[cfg(all(feature = "tokio", feature = "minfac"))]
pub mod prelude {
    pub use crate::device::ActorErrorResultExtensions;
    pub use crate::device::ServiceBuilderExtensions as DeviceServiceBuilderExtensions;
    pub use crate::hosted_service::ServiceBuilderExtensions as HostedServiceServiceServiceBuilderExtensions;
    pub use crate::hosted_service::ServiceCollectionExtensions as HostedServiceServiceCollectionExtensions;
}

#[cfg(feature = "minfac")]
pub extern "C" fn register(collection: &mut minfac::ServiceCollection) {
    crate::device::register_services(collection);
}
