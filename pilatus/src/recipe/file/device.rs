use anyhow::anyhow;
use bytes::Bytes;

use crate::{
    device::{ActorDevice, ActorError, ActorMessage},
    recipe::file::RelativeFilePath,
    FileService, FileServiceExt, RelativeDirectoryPathBuf, TransactionError,
};

#[derive(Debug, Clone)]
pub struct GetFileMessage {
    pub path: RelativeFilePath,
}
impl ActorMessage for GetFileMessage {
    type Output = Vec<u8>;
    type Error = TransactionError;
}

#[derive(Debug, Clone)]
pub struct DeleteFileMessage {
    pub path: RelativeFilePath,
}
impl ActorMessage for DeleteFileMessage {
    type Output = ();
    type Error = TransactionError;
}

#[derive(Debug, Clone)]
pub struct AddFileMessage {
    pub path: RelativeFilePath,
    pub data: Bytes,
}
impl ActorMessage for AddFileMessage {
    type Output = ();
    type Error = anyhow::Error;
}

#[derive(Debug, Clone)]
pub struct ListFilesMessage {
    pub path: RelativeDirectoryPathBuf,
}
impl ActorMessage for ListFilesMessage {
    type Output = Vec<RelativeFilePath>;
    type Error = TransactionError;
}

pub trait RegisterFileHandlersExtension {
    fn add_file_handlers(self) -> Self;
}

impl<T: AsMut<FileService<T>> + AsRef<FileService<T>> + Send + Sync + 'static>
    RegisterFileHandlersExtension for ActorDevice<T>
{
    fn add_file_handlers(self) -> Self {
        async fn get_file<T: AsMut<FileService<T>> + Send + 'static>(
            state: &mut T,
            msg: GetFileMessage,
        ) -> Result<Vec<u8>, ActorError<TransactionError>> {
            state
                .as_mut()
                .get_file(&msg.path)
                .await
                .map_err(ActorError::Custom)
        }

        async fn add_file<
            T: AsMut<FileService<T>> + AsRef<FileService<T>> + Sync + Send + 'static,
        >(
            state: &mut T,
            msg: AddFileMessage,
        ) -> Result<(), ActorError<anyhow::Error>> {
            {
                if !state.has_validator_for(&msg.path) {
                    return Err(ActorError::custom(anyhow!("Access denied")));
                }
            }

            FileServiceExt::add_file_validated(state, &msg.path, &msg.data[..])
                .await
                .map_err(ActorError::custom)
        }

        async fn delete_file<
            T: AsMut<FileService<T>> + AsRef<FileService<T>> + Send + Sync + 'static,
        >(
            state: &mut T,
            msg: DeleteFileMessage,
        ) -> Result<(), ActorError<TransactionError>> {
            {
                if !state.has_validator_for(&msg.path) {
                    return Err(ActorError::custom(anyhow!("Access denied")));
                }
            }
            state
                .as_mut()
                .remove_file(&msg.path)
                .await
                .map_err(ActorError::Custom)
        }

        async fn list_files<T: AsMut<FileService<T>> + Send + 'static>(
            state: &mut T,
            ListFilesMessage { path }: ListFilesMessage,
        ) -> Result<Vec<RelativeFilePath>, ActorError<TransactionError>> {
            state
                .as_mut()
                .list_files(&path)
                .await
                .map_err(ActorError::Custom)
        }

        self.add_handler(get_file)
            .add_handler(add_file)
            .add_handler(delete_file)
            .add_handler(list_files)
    }
}
