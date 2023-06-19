use std::fmt::Debug;

use futures::future::BoxFuture;
use minfac::WeakServiceProvider;
use tokio::task::JoinHandle;

use pilatus::{
    device::{DeviceContext, DeviceResult, SpawnError},
    TransactionError, UpdateParamsMessageError,
};

pub trait DeviceActions: Debug + Send + Sync {
    fn spawn(
        &self,
        device_type: &str,
        ctx: DeviceContext,
        provider: WeakServiceProvider,
    ) -> BoxFuture<Result<JoinHandle<DeviceResult>, StartDeviceError>>;
    fn validate(
        &self,
        device_type: &str,
        ctx: DeviceContext,
    ) -> BoxFuture<Result<(), TransactionError>>;
    fn try_apply(
        &self,
        device_type: &str,
        ctx: DeviceContext,
    ) -> BoxFuture<Result<(), TransactionError>>;
}

#[derive(Debug, thiserror::Error)]
pub enum StartDeviceError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Validation(#[from] UpdateParamsMessageError),
    #[error("Unknown DeviceType")]
    UnknownDeviceType,
}

impl From<SpawnError> for StartDeviceError {
    fn from(value: SpawnError) -> Self {
        match value {
            SpawnError::Io(io) => io.into(),
            SpawnError::Validation(v) => v.into(),
        }
    }
}
