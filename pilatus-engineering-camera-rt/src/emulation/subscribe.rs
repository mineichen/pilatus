use std::sync::Arc;

use futures::StreamExt;
use pilatus::{device::ActorResult, MissedItemsError};
use pilatus_engineering::image::{StreamImageError, SubscribeDynamicImageMessage};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use super::{publish_frame::PublishImageMessage, DeviceState};

impl DeviceState {
    pub(super) async fn subscribe(
        &mut self,
        _msg: SubscribeDynamicImageMessage,
    ) -> ActorResult<SubscribeDynamicImageMessage> {
        if Arc::weak_count(&self.publisher) == 0 {
            self.publisher
                .self_sender
                .clone()
                .tell(PublishImageMessage(Arc::downgrade(&self.publisher)))
                .ok();
        }
        Ok(
            tokio_stream::wrappers::BroadcastStream::new(self.stream.subscribe())
                .map(|r| {
                    r.map_err(|BroadcastStreamRecvError::Lagged(e)| {
                        StreamImageError::MissedItems(MissedItemsError::new(std::num::Saturating(
                            e.min(u16::MAX as u64) as u16,
                        )))
                    })?
                })
                .boxed(),
        )
    }
}
