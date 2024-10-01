use std::{collections::BinaryHeap, sync::Weak, time::Duration};

use futures::StreamExt;
use pilatus::{
    device::{ActorMessage, HandlerResult, Step2, WeakUntypedActorMessageSender},
    RelativeDirPath, RelativeFilePath,
};
use pilatus_engineering::image::{DynamicImage as PilatusDynamicImage, ImageWithMeta};
use tracing::warn;

use super::{DeviceState, Params};

pub(super) struct PublishImageMessage(pub Weak<PublisherState>);

impl ActorMessage for PublishImageMessage {
    type Output = ();
    type Error = ();
}

impl DeviceState {
    pub(super) async fn publish_frame(
        &mut self,
        msg: PublishImageMessage,
    ) -> impl HandlerResult<PublishImageMessage> {
        let re_schedule = if let Some(strong) = msg.0.upgrade() {
            match strong.next_image(self).await {
                Ok(image) => {
                    self.counter += 1;
                    self.stream
                        .send(Ok(ImageWithMeta::with_hash(image, None)))
                        .ok()
                        .map(|_| msg.0)
                }
                Err(e) => {
                    warn!("Stop due to acquisition error: {e:?}");
                    None
                }
            }
        } else {
            None
        };

        Step2(async move {
            if let Some(weak) = re_schedule {
                PublisherState::send_delayed(weak).await;
            }
            Ok(())
        })
    }
}

#[derive(Clone)]
pub(super) struct PublisherState {
    pub params: Params,
    pub self_sender: WeakUntypedActorMessageSender,
}

impl PublisherState {
    pub async fn send_delayed(weak: Weak<Self>) {
        if let Some(state) = weak.upgrade() {
            tokio::time::sleep(Duration::from_millis(state.params.interval)).await;
            state
                .self_sender
                .clone()
                .tell(PublishImageMessage(weak))
                .ok();
        }
    }
    async fn next_image(
        &self,
        state: &mut super::DeviceState,
    ) -> anyhow::Result<PilatusDynamicImage> {
        let files = state
            .file_service
            .stream_files(&RelativeDirPath::root())
            .filter_map(|x| async {
                let entry = x.ok()?;

                (entry.file_name().ends_with(&self.params.file_ending))
                    .then_some(ExistingDirEntry(entry))
            })
            .collect::<BinaryHeap<_>>()
            .await;
        let mut iter = files.iter();
        let first = iter.next();
        let current = match (
            first,
            files
                .iter()
                .skip(state.counter.saturating_sub(1) as usize)
                .next(),
        ) {
            (_, Some(x)) => x,
            (Some(x), _) => {
                state.counter = 0;
                x
            }
            _ => return Err(anyhow::anyhow!("Stop streaming, there is no file")),
        };

        let image_data = state
            .file_service
            .get_file(&RelativeFilePath::new(current.0.file_name())?)
            .await?;
        let img =
            tokio::task::spawn_blocking(move || image::load_from_memory(&image_data)).await??;

        Ok(img.try_into()?)
    }
}

struct ExistingDirEntry(RelativeFilePath);

impl PartialEq for ExistingDirEntry {
    fn eq(&self, other: &Self) -> bool {
        self.0.file_name() == other.0.file_name()
    }
}
impl Eq for ExistingDirEntry {}

impl PartialOrd for ExistingDirEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.0.file_name().partial_cmp(other.0.file_name())
    }
}

impl Ord for ExistingDirEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.file_name().cmp(&other.0.file_name())
    }
}
