use futures::StreamExt;
use minfac::ServiceCollection;
use pilatus::{
    Name, RelativeDirectoryPath,
    device::{ActorMessage, ActorResult, ActorSystem, DynamicIdentifier},
};
use pilatus_axum::{
    DeviceJsonResponse, IntoResponse, ServiceCollectionExtensions,
    extract::{InjectRegistered, Query},
};

use super::DeviceState;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.register_web(crate::device::DEVICE_TYPE, |r| {
        r.http("/collection", |f| f.get(list_collections_web))
    })
}

pub(super) struct ListCollectionsMessage;

impl ActorMessage for ListCollectionsMessage {
    type Output = Vec<Name>;
    type Error = anyhow::Error;
}

impl DeviceState {
    pub(super) async fn list_collections(
        &mut self,
        _msg: ListCollectionsMessage,
    ) -> ActorResult<ListCollectionsMessage> {
        Ok(self
            .file_service
            .stream_directories(RelativeDirectoryPath::root())
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .filter_map(|x| x.ok().and_then(|p| Name::new(p.to_str()?).ok()))
            .collect::<Vec<_>>())
    }
}

async fn list_collections_web(
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
    Query(id): Query<DynamicIdentifier>,
) -> impl IntoResponse {
    DeviceJsonResponse(actor_system.ask(id, ListCollectionsMessage).await)
}
