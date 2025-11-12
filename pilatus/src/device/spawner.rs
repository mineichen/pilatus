use std::{any::Any, fmt::Debug, future::Future, sync::Arc};

use async_trait::async_trait;
use futures_util::{future::BoxFuture, FutureExt};
use minfac::{Resolvable, ServiceCollection, WeakServiceProvider};
use tokio::task::JoinHandle;
use tracing::error;

use super::{
    ActorError, ActorErrorUnknownDevice, ActorSystem, DeviceContext, DeviceId, DeviceResult,
    DeviceValidationContext, WithInfallibleParamUpdate,
};
use crate::{
    DeviceConfig, NotAppliedError, ParameterUpdate, RecipeId, RecipeServiceTrait,
    UnknownDeviceError, UntypedDeviceParamsWithVariables, UpdateParamsMessageError,
};

#[derive(thiserror::Error, Debug)]
#[error("Error in variable {variable_name}")]
pub struct DeviceSpawnerError {
    variable_name: String,
}

#[derive(Debug, thiserror::Error)]

pub enum UpdateDeviceError {
    #[error("{0}")]
    Validate(#[from] UpdateParamsMessageError),

    #[error("{0}")]
    UnknownDevice(#[from] UnknownDeviceError),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl From<ActorError<NotAppliedError>> for UpdateDeviceError {
    fn from(x: ActorError<NotAppliedError>) -> Self {
        match x {
            ActorError::UnknownDevice(ActorErrorUnknownDevice::UnknownDeviceId {
                device_id,
                ..
            }) => UpdateDeviceError::UnknownDevice(UnknownDeviceError(device_id)),
            ActorError::Custom(x) => UpdateDeviceError::Other(x.0),
            x => UpdateDeviceError::Other(x.into()),
        }
    }
}

impl From<ActorError<UpdateParamsMessageError>> for UpdateDeviceError {
    fn from(actor_error: ActorError<UpdateParamsMessageError>) -> Self {
        match actor_error {
            ActorError::UnknownDevice(ActorErrorUnknownDevice::UnknownDeviceId {
                device_id,
                ..
            }) => UpdateDeviceError::UnknownDevice(UnknownDeviceError(device_id)),
            ActorError::Custom(e) => e.into(),
            e => anyhow::Error::from(e).into(),
        }
    }
}

pub trait DeviceHandler: Send + Sync {
    fn clone_box(&self) -> Box<dyn DeviceHandler>;
    fn spawn(
        &self,
        ctx: DeviceContext,
        provider: WeakServiceProvider,
    ) -> BoxFuture<'_, Result<WithInfallibleParamUpdate<JoinHandle<DeviceResult>>, SpawnError>>;
    fn validate(
        &self,
        ctx: DeviceContext,
    ) -> BoxFuture<'_, Result<WithInfallibleParamUpdate<()>, UpdateParamsMessageError>>;
    fn update(
        &self,
        ctx: DeviceContext,
        actor_system: ActorSystem,
    ) -> BoxFuture<'_, Result<(), UpdateDeviceError>>;
    fn get_device_type(&self) -> &'static str;
    fn register_dummy_dependency(&self, col: &mut ServiceCollection);
}

#[derive(thiserror::Error, Debug)]
pub enum SpawnError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Validation(#[from] UpdateParamsMessageError),
}

impl Clone for Box<dyn DeviceHandler> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

pub(crate) struct DepDeviceHandler<TDep: Resolvable, TFut, TParam> {
    pub device_type: &'static str,
    // Doesn't work with fn like handler, because ValidationContext has a lifetime and GAT's can't be used as trait-objects (25.07.2023)
    // Maybe it could be done using HRTB's.
    validator: Arc<dyn for<'a> ValidatorClosure<'a, TParam> + Send + Sync>,
    handler: fn(DeviceContext, TParam, TDep::ItemPreChecked) -> TFut,
}

impl<TDep: Resolvable, TFut, TParam> Clone for DepDeviceHandler<TDep, TFut, TParam> {
    fn clone(&self) -> Self {
        Self {
            device_type: self.device_type,
            validator: self.validator.clone(),
            handler: self.handler,
        }
    }
}

impl<T, TDep, TFut, TParam> DepDeviceHandler<TDep, TFut, TParam>
where
    TDep: Resolvable,
    TFut: Future<Output = T> + Send,
{
    pub(crate) fn new(
        device_type: &'static str,
        validator: Arc<dyn for<'a> ValidatorClosure<'a, TParam> + Send + Sync>,
        handler: fn(DeviceContext, TParam, TDep::ItemPreChecked) -> TFut,
    ) -> Self {
        Self {
            device_type,
            validator,
            handler,
        }
    }
}

pub trait ValidatorClosure<'a, TParams> {
    fn call(
        &self,
        state: DeviceValidationContext<'a>,
    ) -> BoxFuture<'a, Result<WithInfallibleParamUpdate<TParams>, UpdateParamsMessageError>>;
}

impl<T> WithInfallibleParamUpdate<T> {
    pub fn into_data_if_no_changes(self) -> Option<T> {
        if self.update.is_none() {
            Some(self.data)
        } else {
            None
        }
    }
}

#[async_trait]
pub trait InfallibleParamApplier<T: Send> {
    async fn apply(self, changes: WithInfallibleParamUpdate<T>) -> T;
}

pub struct RecipeServiceParamApplier<'a> {
    pub recipe_id: RecipeId,
    pub device_id: DeviceId,
    pub service: &'a (dyn RecipeServiceTrait + Send + Sync),
}
#[async_trait]
impl<'a, T: Send + 'a> InfallibleParamApplier<T> for &'a mut DeviceConfig {
    async fn apply(self, changes: WithInfallibleParamUpdate<T>) -> T {
        if let Some(parameters) = changes.update {
            self.params = parameters;
        }
        changes.data
    }
}

#[async_trait]
impl<'a, T: Send + 'a> InfallibleParamApplier<T> for RecipeServiceParamApplier<'a> {
    async fn apply(self, changes: WithInfallibleParamUpdate<T>) -> T {
        if let Some(parameters) = changes.update {
            if let Err(e) = self
                .service
                .update_device_params(
                    self.recipe_id,
                    self.device_id,
                    ParameterUpdate {
                        parameters,
                        variables: Default::default(),
                    },
                )
                .await
            {
                tracing::error!(
                    "Couldn't update device params for {}: {e:?}",
                    self.device_id
                );
            }
        }
        changes.data
    }
}

// Used to have multiple return values from validators
pub trait IntoParamValidatorOk<T> {
    fn into_ok(self) -> WithInfallibleParamUpdate<T>;
}

impl<T> IntoParamValidatorOk<T> for WithInfallibleParamUpdate<T> {
    fn into_ok(self) -> WithInfallibleParamUpdate<T> {
        self
    }
}
// Debug limit exists to allow other implementations like (T, Option<ParamUpdate>)
impl<T: Debug> IntoParamValidatorOk<T> for T {
    fn into_ok(self) -> WithInfallibleParamUpdate<T> {
        WithInfallibleParamUpdate {
            data: self,
            update: None,
        }
    }
}

impl<T: Debug> IntoParamValidatorOk<T> for (T, Option<UntypedDeviceParamsWithVariables>) {
    fn into_ok(self) -> WithInfallibleParamUpdate<T> {
        WithInfallibleParamUpdate {
            data: self.0,
            update: self.1,
        }
    }
}

impl<'a, TParams, TFut, TFn, TParamOk> ValidatorClosure<'a, TParams> for TFn
where
    TFut: Future<Output = Result<TParamOk, UpdateParamsMessageError>> + 'a + Send,
    TFn: Fn(DeviceValidationContext<'a>) -> TFut,
    TParamOk: IntoParamValidatorOk<TParams>,
{
    fn call(
        &self,
        ctx: DeviceValidationContext<'a>,
    ) -> BoxFuture<'a, Result<WithInfallibleParamUpdate<TParams>, UpdateParamsMessageError>> {
        ((self)(ctx)).map(|x| x.map(|o| o.into_ok())).boxed()
    }
}

impl<TParam: Any + Send + Sync, TDep, TFut> DeviceHandler for DepDeviceHandler<TDep, TFut, TParam>
where
    TDep: Resolvable + Send + 'static,
    TDep::ItemPreChecked: Send,
    TFut: Future<Output = DeviceResult> + Send + 'static,
{
    fn spawn(
        &self,
        ctx: DeviceContext,
        provider: WeakServiceProvider,
    ) -> BoxFuture<'_, Result<WithInfallibleParamUpdate<JoinHandle<DeviceResult>>, SpawnError>>
    {
        async move {
            let validation = (self.validator)
                .call(DeviceValidationContext {
                    raw: &ctx,
                    enable_autorepair: true,
                    //_file_service_builder: self.file_service_builder.clone(),
                })
                .await?;
            let task = (self.handler)(ctx, validation.data, provider.resolve_unchecked::<TDep>());

            #[cfg(tokio_unstable)]
            let param = {
                tokio::task::Builder::new()
                    .name(&format!("Device: {}", self.device_type))
                    .spawn(task)
            }?;
            #[cfg(not(tokio_unstable))]
            let param = tokio::task::spawn(task);
            Ok(WithInfallibleParamUpdate {
                data: param,
                update: validation.update,
            })
        }
        .boxed()
    }

    fn register_dummy_dependency(&self, col: &mut ServiceCollection) {
        col.with::<TDep>().register(|_| ());
    }

    fn clone_box(&self) -> Box<dyn DeviceHandler> {
        Box::new(self.clone())
    }

    fn validate(
        &self,
        ctx: DeviceContext,
    ) -> BoxFuture<'_, Result<WithInfallibleParamUpdate<()>, UpdateParamsMessageError>> {
        async move {
            let r = self
                .validator
                .call(DeviceValidationContext {
                    raw: &ctx,
                    enable_autorepair: true,
                    // _file_service_builder: self.file_service_builder.clone(),
                })
                .await?;
            Ok(WithInfallibleParamUpdate {
                data: (),
                update: r.update,
            })
        }
        .boxed()
    }

    fn update(
        &self,
        ctx: DeviceContext,
        actor_system: ActorSystem,
    ) -> BoxFuture<'_, Result<(), UpdateDeviceError>> {
        async move {
            let typed_params = self
                .validator
                .call(DeviceValidationContext {
                    enable_autorepair: false,
                    raw: &ctx,
                    //_file_service_builder: self.file_service_builder.clone(),
                })
                .await?;

            if typed_params.update.is_some() {
                error!("This is a bug: Unexpected ParameterUpdate. This should happen on startup/import, not when updating params on running device");
                return Err(UpdateDeviceError::Other(anyhow::anyhow!("Unexpected migration")))
            }

            actor_system
                .ask(
                    ctx.id,
                    crate::UpdateParamsMessage::<TParam>::new(typed_params.data),
                )
                .await
                .map_err(Into::into)
        }
        .boxed()
    }

    fn get_device_type(&self) -> &'static str {
        self.device_type
    }
}
