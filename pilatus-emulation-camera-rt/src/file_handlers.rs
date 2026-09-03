use std::io;

use pilatus::{
    AddFileMessage, DeleteFileMessage, GetFileMessage, ListFilesMessage, RelativeResourcePathBuf,
    device::ActorError,
};

use super::validation::{is_responsible, validate_image};

impl super::DeviceState {
    /// Standard file handlers, so this device supports the common `/api/recipe/file` endpoints.
    ///
    /// Adding and deleting is only allowed for validated collection images (`is_responsible`),
    /// therefore no other files of the device can be modified from outside the device.
    pub(super) async fn get_file(
        &mut self,
        GetFileMessage { path }: GetFileMessage,
    ) -> Result<Vec<u8>, ActorError<io::Error>> {
        Ok(self.file_service.get_file(&path).await?)
    }

    pub(super) async fn add_file(
        &mut self,
        AddFileMessage { path, data }: AddFileMessage,
    ) -> Result<(), ActorError<io::Error>> {
        ensure_responsible(&path)?;
        validate_image(&data).map_err(|e| {
            ActorError::custom(io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
        })?;
        self.file_service.add_file_unchecked(&path, &data).await?;
        Ok(())
    }

    pub(super) async fn delete_file(
        &mut self,
        DeleteFileMessage { path }: DeleteFileMessage,
    ) -> Result<(), ActorError<io::Error>> {
        ensure_responsible(&path)?;
        self.file_service.remove_file(&path).await?;
        Ok(())
    }

    pub(super) async fn list_files(
        &mut self,
        ListFilesMessage { path }: ListFilesMessage,
    ) -> Result<Vec<RelativeResourcePathBuf>, ActorError<io::Error>> {
        Ok(self.file_service.list_files(&path).await?)
    }
}

fn ensure_responsible(path: &RelativeResourcePathBuf) -> Result<(), ActorError<io::Error>> {
    if !is_responsible(path) {
        return Err(ActorError::custom(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Access denied",
        )));
    }
    Ok(())
}
