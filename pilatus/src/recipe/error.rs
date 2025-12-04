use std::fmt::Debug;
use std::io;
use std::path::{Path, PathBuf};

use crate::{RecipeId, RelativeDirectoryPathBuf, UnknownDeviceError, UpdateParamsMessageError};
use sealedstruct::ValidationErrors;

#[derive(thiserror::Error, Debug)]
pub enum TransactionError {
    #[error("Recipe {0} already exists.")]
    RecipeAlreadyExists(RecipeId),

    #[error("Invalid recipe id {0}")]
    UnknownRecipeId(RecipeId),

    #[error("{0}")]
    UnknownDevice(#[from] UnknownDeviceError),

    #[error("File Path {0} not found")]
    UnknownFilePath(PathBuf),

    #[error("Error in Filesystem: {0}")]
    FileSystemError(#[from] io::Error),

    #[error("ValidationError: {0}")]
    InvalidDeviceConfig(ValidationErrors),

    #[error("{0:?}")]
    InvalidVariable(VariableError),

    #[error("Other: {0}")]
    Other(#[from] anyhow::Error),
}

impl TransactionError {
    pub fn from_io_producer(path: &Path) -> impl Fn(io::Error) -> TransactionError + '_ {
        |e| match e.kind() {
            std::io::ErrorKind::NotFound => TransactionError::UnknownFilePath(path.to_path_buf()),
            _ => TransactionError::FileSystemError(e),
        }
    }
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

#[derive(Debug)]
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

impl From<VariableError> for TransactionError {
    fn from(e: VariableError) -> Self {
        TransactionError::InvalidVariable(e)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum DirectoryError {
    #[error("Directory {0} not found")]
    NotFound(RelativeDirectoryPathBuf),
    #[error("Directory {0} is not a directory")]
    NotADirectory(RelativeDirectoryPathBuf),
    #[error("Directory {0} is not a directory")]
    Io(#[source] std::io::Error),
}
