use std::any::Any;

use crate::device::{ActorError, ActorMessage};

#[derive(thiserror::Error, Debug)]
pub enum UpdateParamsMessageError {
    #[error("Invalid field '{path}': {message}")]
    /// Path is the json-selector. E.g. "address.street.number"
    InvalidField { path: &'static str, message: String },
    #[error("unable to parse json to struct: {0}")]
    InvalidFormat(#[from] serde_json::Error),
    #[error("unable to validate data")]
    ValidationError(#[from] sealedstruct::ValidationErrors),
    #[error("Failed to apply the changes! Reason: {0}")]
    NotApplied(anyhow::Error),
    #[error("FileError: {0}")]
    File(String),
    #[error("VariableError: {0}")]
    VariableError(String),
}

#[derive(thiserror::Error, Debug)]
#[error("Not applied: {0}")]
pub struct NotAppliedError(#[from] pub anyhow::Error);

impl From<NotAppliedError> for UpdateParamsMessageError {
    fn from(e: NotAppliedError) -> Self {
        UpdateParamsMessageError::NotApplied(e.0)
    }
}

impl From<serde_json::Error> for ActorError<UpdateParamsMessageError> {
    fn from(e: serde_json::Error) -> Self {
        ActorError::Custom(e.into())
    }
}

#[derive(Debug)]
pub struct UpdateParamsMessage<T> {
    pub params: T,
}

impl<T: Any + Send + Sync> ActorMessage for UpdateParamsMessage<T> {
    type Output = ();
    type Error = NotAppliedError;
}
impl<T> UpdateParamsMessage<T> {
    pub fn new(params: T) -> Self {
        Self { params }
    }
}
