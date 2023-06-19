use std::{fmt::Debug, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use futures::{channel::oneshot, future::BoxFuture};
use minfac::ServiceCollection;
use sealedstruct::Sealable;
use serde::de::DeserializeOwned;

#[cfg(feature = "tokio")]
mod minfac_ext;
#[cfg(feature = "tokio")]
mod spawner;
mod system;

pub type DeviceResult = Result<()>;
#[cfg(feature = "tokio")]
pub use minfac_ext::*;
#[cfg(feature = "tokio")]
pub use spawner::*;
pub use system::*;

use crate::{RecipeId, UntypedDeviceParamsWithoutVariables, UpdateParamsMessageError, Variables};

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
pub struct DeviceValidationContext<'a> {
    pub(super) raw: &'a DeviceContext,
}

impl<'a> DeviceValidationContext<'a> {
    pub fn params_as<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        self.raw.params.params_as::<T>()
    }

    pub fn params_as_sealed<T: DeserializeOwned + Sealable>(
        &self,
    ) -> Result<T::Target, UpdateParamsMessageError>
    where
        T::Target:,
    {
        self.raw
            .params
            .params_as::<T>()
            .map_err(Into::into)
            .and_then(|x| x.seal().map_err(Into::into))
    }
}

#[non_exhaustive]
pub struct DeviceContext {
    pub id: DeviceId,
    variables: Variables,
    params: UntypedDeviceParamsWithoutVariables,
}

impl DeviceContext {
    pub fn new(
        id: DeviceId,
        variables: Variables,
        params: UntypedDeviceParamsWithoutVariables,
    ) -> Self {
        Self {
            id,
            variables,
            params,
        }
    }
    #[cfg(feature = "test")]
    pub fn with_random_id(device: impl serde::Serialize) -> Self {
        Self::new(
            DeviceId::new_v4(),
            Variables::default(),
            UntypedDeviceParamsWithoutVariables::from_serializable(&device).unwrap(),
        )
    }
}

pub trait FinalizeRecipeExecution: Send + Sync {
    fn finalize_recipe_execution(&self) -> BoxFuture<'_, ()>;
}
