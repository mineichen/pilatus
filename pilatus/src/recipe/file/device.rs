use std::io;

use bytes::Bytes;

use crate::{
    device::{ActorDevice, ActorError, ActorMessage},
    recipe::file::RelativeFilePath,
    FileService, FileServiceExt, RelativeDirectoryPathBuf,
};

#[derive(Debug, Clone)]
pub struct GetFileMessage {
    pub path: RelativeFilePath,
}
impl ActorMessage for GetFileMessage {
    type Output = Vec<u8>;
    type Error = io::Error;
}

#[derive(Debug, Clone)]
pub struct DeleteFileMessage {
    pub path: RelativeFilePath,
}
impl ActorMessage for DeleteFileMessage {
    type Output = ();
    type Error = io::Error;
}

#[derive(Debug, Clone)]
pub struct AddFileMessage {
    pub path: RelativeFilePath,
    pub data: Bytes,
}
impl ActorMessage for AddFileMessage {
    type Output = ();
    type Error = io::Error;
}

#[derive(Debug, Clone)]
pub struct ListFilesMessage {
    pub path: RelativeDirectoryPathBuf,
}
impl ActorMessage for ListFilesMessage {
    type Output = Vec<RelativeFilePath>;
    type Error = io::Error;
}

pub trait RegisterFileHandlersExtension {
    fn add_file_handlers(self) -> Self;
}

impl<T> RegisterFileHandlersExtension for ActorDevice<T>
where
    T: AsMut<FileService<T>> + AsRef<FileService<T>> + Send + Sync + 'static,
{
    fn add_file_handlers(self) -> Self {
        async fn get_file<T: AsMut<FileService<T>> + Send + 'static>(
            state: &mut T,
            msg: GetFileMessage,
        ) -> Result<Vec<u8>, ActorError<io::Error>> {
            Ok(state.as_mut().get_file(&msg.path).await?)
        }

        async fn add_file<
            T: AsMut<FileService<T>> + AsRef<FileService<T>> + Sync + Send + 'static,
        >(
            state: &mut T,
            msg: AddFileMessage,
        ) -> Result<(), ActorError<io::Error>> {
            {
                if !state.has_validator_for(&msg.path) {
                    return Err(ActorError::Custom(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "Access denied",
                    )));
                }
            }

            Ok(FileServiceExt::add_file_validated(state, &msg.path, &msg.data[..]).await?)
        }

        async fn delete_file<
            T: AsMut<FileService<T>> + AsRef<FileService<T>> + Send + Sync + 'static,
        >(
            state: &mut T,
            msg: DeleteFileMessage,
        ) -> Result<(), ActorError<io::Error>> {
            {
                if !state.has_validator_for(&msg.path) {
                    return Err(ActorError::Custom(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "Access denied",
                    )));
                }
            }
            Ok(state.as_mut().remove_file(&msg.path).await?)
        }

        async fn list_files<T: AsMut<FileService<T>> + Send + 'static>(
            state: &mut T,
            ListFilesMessage { path }: ListFilesMessage,
        ) -> Result<Vec<RelativeFilePath>, ActorError<io::Error>> {
            Ok(state.as_mut().list_files(&path).await?)
        }

        self.add_handler(get_file)
            .add_handler(add_file)
            .add_handler(delete_file)
            .add_handler(list_files)
    }
}
