use std::sync::Arc;

use imbuf::Image;
use pilatus::device::{HandlerResult, Step2};
use pilatus_engineering::image::{GetImageMessage, ImageWithMeta};
use tracing::trace;

use crate::{DeviceState, publish_frame::PublisherState};

impl DeviceState {
    /// If stream runs, it should get the current image. For slow streams, this allows to use the image the customer currently sees.
    /// If the stream is stopped, it takes the current, but consumes one image from the stream, so the next GetImageMessage returns a different image
    pub async fn handle_get_image(
        &mut self,
        _msg: GetImageMessage,
    ) -> impl HandlerResult<GetImageMessage> {
        let is_streaming = Arc::weak_count(&self.publisher) != 0;
        let move_to_next = !self.paused && !is_streaming;
        trace!("Move to next: {move_to_next:?}");
        let image = PublisherState::next_image_if_upgradeable(
            &Arc::downgrade(&self.publisher),
            self,
            move_to_next,
        )
        .await;

        Step2(async move {
            let (image, _) = image?.ok_or_else(|| anyhow::anyhow!("No more images available"))?;
            let luma_image: Image<u8, 1> = image.try_into().map_err(anyhow::Error::from)?;
            Ok(ImageWithMeta::with_hash(luma_image, None))
        })
    }
}
