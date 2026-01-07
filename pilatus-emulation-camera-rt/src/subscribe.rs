use std::sync::Arc;

use futures::{StreamExt, TryFutureExt};
use pilatus::{
    MissedItemsError,
    device::{ActorErrorResultExtensions, ActorResult},
};
use pilatus_engineering::image::{
    BroadcastImage, LumaImage, StreamImageError, SubscribeDynamicImageMessage,
    SubscribeImageMessage,
};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tracing::{debug, warn};

use super::DeviceState;
use pilatus_emulation_camera::EmulationMode;

impl DeviceState {
    pub(super) async fn subscribe_luma(
        &mut self,
        _msg: SubscribeImageMessage,
    ) -> ActorResult<SubscribeImageMessage> {
        self.subscribe(SubscribeDynamicImageMessage::default())
            .await
            .map(|x| {
                x.filter_map(|x| async move {
                    let dynamic = x.ok()?.image;
                    let luma: LumaImage = dynamic
                        .try_into()
                        .inspect_err(|e| {
                            tracing::error!("Error converting dynamic image to luma image: {e:?}")
                        })
                        .ok()?;
                    Some(BroadcastImage::with_hash(luma, None))
                })
                .boxed()
            })
            .map_actor_error(|_| anyhow::anyhow!("Streaming error"))
    }

    pub(super) async fn subscribe(
        &mut self,
        _msg: SubscribeDynamicImageMessage,
    ) -> ActorResult<SubscribeDynamicImageMessage> {
        match self.publisher.params.mode {
            EmulationMode::File => {
                if Arc::weak_count(&self.publisher) == 0 {
                    debug!("No PublishImageMessage is stored in the queue. Initialize Publishing");
                    if let Err(e) = self.publisher.enqueue(self).await {
                        warn!("Couldn't enqueue publisher: {e:?}");
                    }
                }
                Ok(
                    tokio_stream::wrappers::BroadcastStream::new(self.stream.subscribe())
                        .map(|r| {
                            r.map_err(|BroadcastStreamRecvError::Lagged(e)| {
                                StreamImageError::MissedItems(MissedItemsError::new(
                                    std::num::Saturating(e.min(u16::MAX as u64) as u16),
                                ))
                            })?
                        })
                        .boxed(),
                )
            }
            EmulationMode::Streaming => {
                let params = &self.publisher.params;
                let Some(source_id) = params
                    .permanent_recording
                    .as_ref()
                    .map(|x| x.source_id)
                    .or(params.streaming.source_device_id)
                else {
                    warn!("Souldn't happen (todo: this should be handled during validation)");
                    return Ok(futures::stream::empty().boxed());
                };
                let collection_dir = params
                    .permanent_recording
                    .as_ref()
                    .map(|x| x.collection_path().to_path_buf());
                let file_service = self.file_service.clone();
                let encoder = self.encoder.clone();

                Ok(self
                    .actor_system
                    .ask(source_id, SubscribeDynamicImageMessage::default())
                    .await?
                    .map(move |x| {
                        let collection_dir = collection_dir.clone();
                        let file_service = file_service.clone();
                        let encoder = encoder.clone();
                        async move {
                            let ok = x?;
                            let time = std::time::SystemTime::now();
                            if let Some(collection_dir) = collection_dir
                                && let Err(e) =
                                    super::record::encode_all(ok.clone(), encoder.clone())
                                        .and_then(|x| {
                                            super::record::save_encoded(
                                                (time, x),
                                                file_service,
                                                &collection_dir,
                                            )
                                        })
                                        .await
                            {
                                warn!("Couldn't save streaming image {e}.");
                            }
                            Ok(ok)
                        }
                    })
                    .buffer_unordered(8)
                    .boxed())
            }
        }
    }
}
