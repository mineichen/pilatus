use std::{any::Any, future::Future, io, sync::Arc};

use futures::{future::BoxFuture, FutureExt};
use minfac::{Resolvable, ServiceCollection, WeakServiceProvider};
use tokio::task::JoinHandle;

use crate::{NotAppliedError, UpdateParamsMessageError};

use super::{
    ActorError, ActorErrorUnknownDevice, ActorSystem, DeviceContext, DeviceValidationContext,
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

pub trait DeviceHandler<T>: Send + Sync {
    fn clone_box(&self) -> Box<dyn DeviceHandler<T>>;
    fn spawn(
        &self,
        ctx: DeviceContext,
        provider: WeakServiceProvider,
    ) -> BoxFuture<Result<JoinHandle<T>, SpawnError>>;
    fn validate(&self, ctx: DeviceContext) -> BoxFuture<Result<(), UpdateParamsMessageError>>;
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

impl<T> Clone for Box<dyn DeviceHandler<T>> {
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
    ) -> BoxFuture<'a, Result<TParams, UpdateParamsMessageError>>;
}

impl<'a, TParams, TFut, TFn> ValidatorClosure<'a, TParams> for TFn
where
    TFut: Future<Output = Result<TParams, UpdateParamsMessageError>> + 'a + Send,
    TFn: Fn(DeviceValidationContext<'a>) -> TFut,
{
    fn call(
        &self,
        ctx: DeviceValidationContext<'a>,
    ) -> BoxFuture<'a, Result<TParams, UpdateParamsMessageError>> {
        ((self)(ctx)).boxed()
    }
}

impl<T: 'static + Send, TParam: Any + Send + Sync, TDep, TFut> DeviceHandler<T>
    for DepDeviceHandler<TDep, TFut, TParam>
where
    TDep: Resolvable + Send + 'static,
    TDep::ItemPreChecked: Send,
    TFut: Future<Output = T> + Send + 'static,
{
    fn spawn(
        &self,
        ctx: DeviceContext,
        provider: WeakServiceProvider,
    ) -> BoxFuture<Result<JoinHandle<T>, SpawnError>> {
        // Todo: IO-Error is not optimal here
        async move {
            let param = (self.validator)
                .call(DeviceValidationContext {
                    raw: &ctx,
                    //_file_service_builder: self.file_service_builder.clone(),
                })
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            let task = (self.handler)(ctx, param, provider.resolve_unchecked::<TDep>());
            tokio::task::Builder::new()
                .name(&format!("Device {}", self.device_type))
                .spawn(task)
                .map_err(Into::into)
        }
        .boxed()
    }

    fn register_dummy_dependency(&self, col: &mut ServiceCollection) {
        col.with::<TDep>().register(|_| ());
    }

    fn clone_box(&self) -> Box<dyn DeviceHandler<T>> {
        Box::new(self.clone())
    }

    fn validate(&self, ctx: DeviceContext) -> BoxFuture<Result<(), UpdateParamsMessageError>> {
        async move {
            self.validator
                .call(DeviceValidationContext {
                    raw: &ctx,
                    // _file_service_builder: self.file_service_builder.clone(),
                })
                .await?;
            Ok(())
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
                    raw: &ctx,
                    //_file_service_builder: self.file_service_builder.clone(),
                })
                .await?;
            actor_system
                .ask(ctx.id, crate::UpdateParamsMessage::new(typed_params))
                .await
                .map_err(Into::into)
        }
        .boxed()
    }

    fn get_device_type(&self) -> &'static str {
        self.device_type
    }
}
