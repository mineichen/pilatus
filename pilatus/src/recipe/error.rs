use std::fmt::Debug;
use std::io;

use crate::{
    RecipeAlreadyExistsError, RecipeId, UnknownDeviceError, UnknownRecipeError,
    UpdateParamsMessageError,
};
use sealedstruct::ValidationErrors;

#[derive(thiserror::Error, Debug)]
pub enum TransactionError {
    #[error(transparent)]
    RecipeAlreadyExists(#[from] RecipeAlreadyExistsError),

    #[error(transparent)]
    UnknownRecipeId(#[from] UnknownRecipeError),

    #[error(transparent)]
    UnknownDevice(#[from] UnknownDeviceError),

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    InvalidDeviceConfig(#[from] ValidationErrors),

    #[error(transparent)]
    InvalidVariable(#[from] VariableError),

    #[error("Other: {0}")]
    Other(#[from] anyhow::Error),
}

impl TransactionError {
    pub fn other(e: impl Into<anyhow::Error>) -> Self {
        TransactionError::Other(e.into())
    }
}
impl From<UpdateParamsMessageError> for TransactionError {
    fn from(x: UpdateParamsMessageError) -> Self {
        match x {
            UpdateParamsMessageError::ValidationError(e) => {
                TransactionError::InvalidDeviceConfig(e)
            }
            e => TransactionError::Other(e.into()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Error in Recipe {recipe_id}: {reason}")]
pub struct VariableError {
    pub recipe_id: RecipeId,
    pub reason: anyhow::Error,
}

impl<T: Into<anyhow::Error>> From<(RecipeId, T)> for VariableError {
    fn from((recipe_id, reason): (RecipeId, T)) -> Self {
        VariableError {
            recipe_id,
            reason: reason.into(),
        }
    }
}
