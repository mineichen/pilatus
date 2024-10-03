use std::{fmt::Debug, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use futures::{channel::oneshot, future::BoxFuture};

use crate::{RecipeId, UntypedDeviceParamsWithVariables, Variables};

mod active_state;
#[cfg(all(feature = "tokio", feature = "minfac"))]
mod minfac_ext;
#[cfg(all(feature = "tokio", feature = "minfac"))]
mod spawner;
mod system;
#[cfg(feature = "tokio")]
mod validation;

pub use active_state::*;
pub type DeviceResult = Result<()>;
#[cfg(all(feature = "tokio", feature = "minfac"))]
pub use minfac_ext::*;
#[cfg(all(feature = "tokio", feature = "minfac"))]
pub use spawner::*;
pub use system::*;
#[cfg(feature = "tokio")]
pub use validation::*;

#[cfg(feature = "minfac")]
pub(super) fn register_services(c: &mut minfac::ServiceCollection) {
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
#[allow(dead_code)]
pub struct DeviceContext {
    pub id: DeviceId,
    // Must stay private to forbid access to variables in device
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

/// To get to the inner data, the change has to be applied with something that implements InfallibleParamApplier
#[must_use]
pub struct WithInfallibleParamUpdate<T> {
    pub(crate) data: T,
    /// Doesn't use `ParameterUpdate` on purpose to ensure conflict-free migration. But this should be allowed in the future (this is why update must stay private)
    /// Idea: Rename variables with conflicts, so a conflict-free migration is possible
    /// -> device-restart is not needed, as the configuration would result in same result
    /// -> Have a wizzard to resolve conflicts (reunite previously linked variables)
    pub(crate) update: Option<UntypedDeviceParamsWithVariables>,
}

/// Allows intercepting recipe shutdown
/// When all devices shut down, services like axum have the opportunity to close all Websockets related to a device
/// before the next recipe is started.
/// If they didn't the ServiceProvider might get dropped while WeakServiceProvider references exist, resulting in an error.
pub trait FinalizeRecipeExecution: Send + Sync {
    fn finalize_recipe_execution(&self) -> BoxFuture<'_, ()>;
}
