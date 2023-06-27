use std::{any::Any, fmt::Debug, future::Future, sync::Arc};

use async_trait::async_trait;
use futures::{future::BoxFuture, FutureExt};
use minfac::{Resolvable, ServiceCollection, WeakServiceProvider};
use tokio::task::JoinHandle;
use tracing::error;

use super::{
    ActorError, ActorErrorUnknownDevice, ActorSystem, DeviceContext, DeviceId, DeviceResult,
    DeviceValidationContext,
};
use crate::{
    NotAppliedError, ParameterUpdate, RecipeId, RecipeServiceTrait,
    UntypedDeviceParamsWithVariables, UpdateParamsMessageError,
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
    UnknownDevice(#[from] ActorErrorUnknownDevice),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl From<ActorError<NotAppliedError>> for UpdateDeviceError {
    fn from(x: ActorError<NotAppliedError>) -> Self {
        match x {
            ActorError::UnknownDevice(x) => x.into(),
            ActorError::Custom(x) => UpdateDeviceError::Other(x.0),
            x => UpdateDeviceError::Other(x.into()),
        }
    }
}

impl From<ActorError<UpdateParamsMessageError>> for UpdateDeviceError {
    fn from(actor_error: ActorError<UpdateParamsMessageError>) -> Self {
        match actor_error {
            ActorError::UnknownDevice(e) => e.into(),
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
    ) -> BoxFuture<Result<WithInfallibleParamUpdate<JoinHandle<DeviceResult>>, SpawnError>>;
    fn validate(
        &self,
        ctx: DeviceContext,
    ) -> BoxFuture<Result<Option<UntypedDeviceParamsWithVariables>, UpdateParamsMessageError>>;
    fn update(
        &self,
        ctx: DeviceContext,
        actor_system: ActorSystem,
    ) -> BoxFuture<Result<(), UpdateDeviceError>>;
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

trait BoxFnWrapperTrait<'a, TParam, TFut: Future> {
    fn call(&self, param: &'a TParam) -> BoxFuture<'a, TFut::Output>;
}
struct BoxFnWrapper<'a, TParam, TFut>(fn(&'a TParam) -> TFut);

impl<'a, TParam, TFut: Future + Send + 'a> BoxFnWrapperTrait<'a, TParam, TFut>
    for BoxFnWrapper<'a, TParam, TFut>
{
    fn call(&self, param: &'a TParam) -> BoxFuture<'a, TFut::Output> {
        (self.0)(param).boxed()
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

/// To get to the inner data, the change has to be applied with Something that implements InfallibleParamApplier
pub struct WithInfallibleParamUpdate<T> {
    pub(crate) data: T,
    /// Doesn't use `ParameterUpdate` on purpose to ensure conflict-free migration. But this should be allowed in the future (this is why update must stay private)
    /// Idea: Rename variables with conflicts, so a conflict-free migration is possible
    /// -> device-restart is not needed, as the configuration would result in same result
    /// -> Have a wizzard to resolve conflicts (reunite previously linked variables)
    pub(crate) update: Option<UntypedDeviceParamsWithVariables>,
}

#[async_trait]
pub trait InfallibleParamApplier<T: Send> {
    async fn apply(
        &mut self,
        recipe_id: &RecipeId,
        device_id: DeviceId,
        x: WithInfallibleParamUpdate<T>,
    ) -> T;
}

#[async_trait]
impl<'a, T: Send + 'a> InfallibleParamApplier<T> for &'a (dyn RecipeServiceTrait + Send + Sync) {
    async fn apply(
        &mut self,
        recipe_id: &RecipeId,
        device_id: DeviceId,
        x: WithInfallibleParamUpdate<T>,
    ) -> T {
        if let Some(parameters) = x.update {
            if let Err(e) = self
                .update_device_params(
                    recipe_id.clone(),
                    device_id,
                    ParameterUpdate {
                        parameters,
                        variables: Default::default(),
                    },
                    Default::default(),
                )
                .await
            {
                tracing::error!("Couldn't update device params for {device_id}: {e:?}");
            }
        }
        x.data
    }
}

#[cfg(feature = "test")]
#[async_trait]
impl<'a, T: Send + 'static> InfallibleParamApplier<T> for &'a mut u32 {
    async fn apply(
        &mut self,
        _recipe_id: &RecipeId,
        _device_id: DeviceId,
        x: WithInfallibleParamUpdate<T>,
    ) -> T {
        if x.update.is_some() {
            **self += 1;
        }
        x.data
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
    ) -> BoxFuture<Result<WithInfallibleParamUpdate<JoinHandle<DeviceResult>>, SpawnError>> {
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
    ) -> BoxFuture<Result<Option<UntypedDeviceParamsWithVariables>, UpdateParamsMessageError>> {
        async move {
            let r = self
                .validator
                .call(DeviceValidationContext {
                    raw: &ctx,
                    enable_autorepair: true,
                    // _file_service_builder: self.file_service_builder.clone(),
                })
                .await?;
            Ok(r.update)
        }
        .boxed()
    }

    fn update(
        &self,
        ctx: DeviceContext,
        actor_system: ActorSystem,
    ) -> BoxFuture<Result<(), UpdateDeviceError>> {
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
