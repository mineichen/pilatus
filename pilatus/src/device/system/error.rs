use std::{borrow::Cow, fmt::Debug};

use futures::{channel::oneshot, stream::Aborted};

use crate::device::DeviceId;

use super::ActorMessage;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ActorError<TCustom: Debug> {
    #[error("{0}")]
    UnknownDevice(#[from] ActorErrorUnknownDevice),

    #[error("Message cannot be processed by this device: {0}")]
    UnknownMessageType(&'static str),

    #[error("Error occured within the device: {0:?}")]
    Custom(TCustom),

    #[error("Too much load on the system: {0}")]
    Busy(#[from] ActorErrorBusy),

    #[error("Request was aborted by it's handler.")]
    Aborted,

    #[error("Running into timeout when handling request")]
    Timeout,
}

impl<T: Debug> From<Aborted> for ActorError<T> {
    fn from(_: Aborted) -> Self {
        ActorError::Aborted
    }
}

impl<T: Debug> From<oneshot::Canceled> for ActorError<T> {
    fn from(_: oneshot::Canceled) -> Self {
        ActorError::Aborted
    }
}

#[cfg(any(feature = "tokio", test))]
impl<T: Debug> From<tokio::time::error::Elapsed> for ActorError<T> {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        ActorError::Timeout
    }
}

pub type ActorResult<TMsg> =
    Result<<TMsg as ActorMessage>::Output, ActorError<<TMsg as ActorMessage>::Error>>;

impl From<anyhow::Error> for ActorError<anyhow::Error> {
    fn from(e: anyhow::Error) -> Self {
        ActorError::Custom(e)
    }
}

impl From<()> for ActorError<()> {
    fn from(i: ()) -> Self {
        ActorError::Custom(i)
    }
}

#[cfg(any(feature = "tokio", test))]
impl<T: Debug> From<tokio::task::JoinError> for ActorError<T> {
    fn from(_: tokio::task::JoinError) -> Self {
        ActorError::Busy(ActorErrorBusy::SpawnBlocking)
    }
}

impl<TCustom: Debug> ActorError<TCustom> {
    pub fn map_custom<TCustomNew: Debug>(
        self,
        mapper: impl FnOnce(TCustom) -> TCustomNew,
    ) -> ActorError<TCustomNew> {
        match self {
            ActorError::UnknownDevice(x) => ActorError::UnknownDevice(x),
            ActorError::UnknownMessageType(x) => ActorError::UnknownMessageType(x),
            ActorError::Custom(x) => ActorError::Custom(mapper(x)),
            ActorError::Busy(x) => ActorError::Busy(x),
            ActorError::Aborted => ActorError::Aborted,
            ActorError::Timeout => ActorError::Timeout,
        }
    }
    pub fn custom(custom: impl Into<TCustom>) -> Self {
        Self::Custom(custom.into())
    }
}

impl<T: Debug> From<ActorWeakTellError> for ActorError<T> {
    fn from(value: ActorWeakTellError) -> Self {
        match value {
            ActorWeakTellError::UnknownDevice(x) => x.into(),
            ActorWeakTellError::Busy(x) => x.into(),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ActorWeakTellError {
    #[error("{0}")]
    UnknownDevice(#[from] ActorErrorUnknownDevice),

    #[error("Too much load on the system: {0}")]
    Busy(#[from] ActorErrorBusy),
}

pub trait ActorErrorResultExtensions<T, TErr: Debug> {
    fn map_actor_error<TErrNew: Debug>(
        self,
        mapper: fn(TErr) -> TErrNew,
    ) -> Result<T, ActorError<TErrNew>>;
}

impl<T, TErr: Debug> ActorErrorResultExtensions<T, TErr> for Result<T, ActorError<TErr>> {
    fn map_actor_error<TErrNew: Debug>(
        self,
        mapper: fn(TErr) -> TErrNew,
    ) -> Result<T, ActorError<TErrNew>> {
        self.map_err(|e| e.map_custom(mapper))
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("Unknown device with id '{device_id}'. Reason: {detail}")]
pub struct ActorErrorUnknownDevice {
    pub device_id: DeviceId,
    pub detail: Cow<'static, str>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ActorErrorBusy {
    #[error("Queue for device {0} has no more space")]
    ExceededQueueCapacity(DeviceId),

    #[error("spawn_blocking failed due to system overload")]
    SpawnBlocking,
}
