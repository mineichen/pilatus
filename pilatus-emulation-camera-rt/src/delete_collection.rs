use minfac::ServiceCollection;
use pilatus::{
    DirectoryError, Name, RelativeDirectoryPath,
    device::{ActorError, ActorMessage, ActorResult, ActorSystem, DynamicIdentifier},
};
use pilatus_axum::{
    DeviceResponse, IntoResponse, ServiceCollectionExtensions,
    extract::{InjectRegistered, Path, Query},
};

use super::DeviceState;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.register_web(crate::device::DEVICE_TYPE, |r| {
        r.http("/collection/{collection_name}", |f| {
            f.delete(delete_collection_web)
        })
    })
}

pub(super) struct DeleteCollectionMessage {
    pub collection_name: Name,
}

impl ActorMessage for DeleteCollectionMessage {
    type Output = ();
    type Error = DirectoryError;
}

impl DeviceState {
    pub(super) async fn delete_collection(
        &mut self,
        msg: DeleteCollectionMessage,
    ) -> ActorResult<DeleteCollectionMessage> {
        let collection_path = RelativeDirectoryPath::new(msg.collection_name.as_str())
            .map_err(|e| ActorError::Custom(DirectoryError::Io(std::io::Error::other(e))))?;

        self.file_service
            .remove_directory(&collection_path)
            .await
            .map_err(ActorError::custom)?;

        Ok(())
    }
}

async fn delete_collection_web(
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
    Path(collection_name): Path<Name>,
    Query(id): Query<DynamicIdentifier>,
) -> impl IntoResponse {
    DeviceResponse::from(
        actor_system
            .ask(id, DeleteCollectionMessage { collection_name })
            .await,
    )
}
