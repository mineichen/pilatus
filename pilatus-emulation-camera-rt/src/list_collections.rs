use futures::StreamExt;
use pilatus::{
    Name, RelativeDirectoryPath,
    device::{ActorMessage, ActorResult},
};

use super::DeviceState;

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
