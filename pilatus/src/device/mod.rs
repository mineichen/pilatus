use std::{fmt::Debug, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use futures::{channel::oneshot, future::BoxFuture};
use minfac::ServiceCollection;

use crate::{RecipeId, UntypedDeviceParamsWithVariables, UpdateParamsMessageError, Variables};

#[cfg(feature = "tokio")]
mod minfac_ext;
#[cfg(feature = "tokio")]
mod spawner;
mod system;
#[cfg(feature = "tokio")]
mod validation;

pub type DeviceResult = Result<()>;
#[cfg(feature = "tokio")]
pub use minfac_ext::*;
#[cfg(feature = "tokio")]
pub use spawner::*;
pub use system::*;
#[cfg(feature = "tokio")]
pub use validation::*;

pub(super) fn register_services(c: &mut ServiceCollection) {
    system::register_services(c);
}

crate::uuid_wrapper::wrapped_uuid!(DeviceId);

#[derive(Debug)]
pub struct IgnoreNotSendableOneShotChannel<T> {
    one_shot: oneshot::Sender<T>,
}

#[derive(Clone)]
pub struct RecipeRunner(Arc<dyn RecipeRunnerTrait>);

impl RecipeRunner {
    pub fn new(inner: Arc<dyn RecipeRunnerTrait>) -> Self {
        Self(inner)
    }

    pub fn select_recipe(&self, recipe_id: RecipeId) -> BoxFuture<anyhow::Result<()>> {
        self.0.select_recipe(recipe_id)
    }
}

#[async_trait]
pub trait RecipeRunnerTrait: Send + Sync {
    async fn select_recipe(&self, recipe_id: RecipeId) -> anyhow::Result<()>;
}

impl<T> IgnoreNotSendableOneShotChannel<T>
where
    T: Debug + Send + Sync + 'static,
{
    pub fn send(self, m: T) {
        let _ignore_error = self.one_shot.send(m);
    }
}

#[non_exhaustive]
pub struct DeviceContext {
    pub id: DeviceId,
    variables: Variables,
    params_with_vars: UntypedDeviceParamsWithVariables,
}

impl DeviceContext {
    pub fn new(
        id: DeviceId,
        variables: Variables,
        params_with_vars: UntypedDeviceParamsWithVariables,
    ) -> Self {
        Self {
            id,
            variables,
            params_with_vars,
        }
    }
    #[cfg(feature = "unstable")]
    pub fn with_random_id(device: impl serde::Serialize) -> Self {
        Self::new(
            DeviceId::new_v4(),
            Variables::default(),
            UntypedDeviceParamsWithVariables::new(serde_json::to_value(&device).unwrap()),
        )
    }
}

/// Allows intercepting recipe shutdown
/// When all devices shut down, services like axum have the opportunity to close all Websockets related to a device
/// before the next recipe is started.
/// If they didn't the ServiceProvider might get dropped while WeakServiceProvider references exist, resulting in an error.
pub trait FinalizeRecipeExecution: Send + Sync {
    fn finalize_recipe_execution(&self) -> BoxFuture<'_, ()>;
}
