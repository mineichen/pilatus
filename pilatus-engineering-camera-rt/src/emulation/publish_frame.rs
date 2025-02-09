use std::{collections::BinaryHeap, sync::Weak, time::Duration};

use futures::{StreamExt, TryStreamExt};
use pilatus::{
    device::{ActorMessage, HandlerResult, Step2, WeakUntypedActorMessageSender},
    RelativeDirectoryPath, RelativeDirectoryPathBuf, RelativeFilePath, TransactionError,
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
                Ok(image) => self
                    .stream
                    .send(Ok(ImageWithMeta::with_hash(image, None)))
                    .ok()
                    .map(|_| msg.0),
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

    pub async fn get_collection_directory(
        &self,
        state: &super::DeviceState,
    ) -> Result<RelativeDirectoryPathBuf, TransactionError> {
        let mut all = state
            .file_service
            .stream_directories(RelativeDirectoryPath::root());
        let maybe = match self.params.active.as_ref() {
            Some(name) => {
                std::pin::pin!(all.try_filter_map(|x| async move {
                    Ok(match x.file_name() {
                        Some(filename) if filename == name.as_str() => Some(x),
                        _ => None,
                    })
                }))
                .next()
                .await
            }
            None => all.next().await,
        };
        match maybe {
            Some(x) => x,
            None => Err(TransactionError::Other(anyhow::anyhow!(
                "No collection exists"
            ))),
        }
        //        RelativeDirectoryPath::new(self.params.)
    }

    // Todo: Cache list instead of getting it in each acquisition (e.g. only get it when run is over to accomodate for new images without restart)
    async fn next_image(
        &self,
        state: &mut super::DeviceState,
    ) -> anyhow::Result<PilatusDynamicImage> {
        let files = state
            .file_service
            .stream_files_recursive(&self.get_collection_directory(state).await?)
            .filter_map(|x| async {
                let entry = x.ok()?;

                (entry.file_name().ends_with(&self.params.file_ending))
                    .then_some(ExistingCollectionEntry(entry))
            })
            .collect::<BinaryHeap<_>>()
            .await;
        let first = files.peek();
        let (current, new_count) = match (first, files.iter().nth(state.counter as usize)) {
            (_, Some(x)) => (x, state.counter + 1),
            (Some(x), _) => (x, 1),
            _ => return Err(anyhow::anyhow!("Stop streaming, there is no file")),
        };
        if !state.paused {
            state.counter = new_count;
        }

        let image_data = state.file_service.get_file(&current.0).await?;
        let img =
            tokio::task::spawn_blocking(move || image::load_from_memory(&image_data)).await??;

        Ok(img.try_into()?)
    }
}

struct ExistingCollectionEntry(RelativeFilePath);

impl PartialEq for ExistingCollectionEntry {
    fn eq(&self, other: &Self) -> bool {
        self.0.file_name() == other.0.file_name()
    }
}
impl Eq for ExistingCollectionEntry {}

impl PartialOrd for ExistingCollectionEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ExistingCollectionEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.file_name().cmp(other.0.file_name())
    }
}
