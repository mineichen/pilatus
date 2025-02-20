use std::{collections::BinaryHeap, path::PathBuf, sync::Weak, time::Duration};

use futures::{StreamExt, TryStreamExt};
use pilatus::{
    device::{ActorMessage, HandlerResult, Step2, WeakUntypedActorMessageSender},
    RelativeDirectoryPath, TransactionError,
};
use pilatus_engineering::image::{DynamicImage as PilatusDynamicImage, ImageWithMeta};
use tracing::{debug, warn};

use super::{ActiveRecipe, DeviceState, Params};

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

pub(super) struct PublisherState {
    pub params: Params,
    pub self_sender: WeakUntypedActorMessageSender,
    pub pending_active: tokio::sync::Mutex<BinaryHeap<ExistingCollectionEntry>>,
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
    ) -> Result<PathBuf, TransactionError> {
        let mut all = state
            .file_service
            .stream_directories(RelativeDirectoryPath::root());
        let maybe = match &self.params.active {
            ActiveRecipe::Undefined => all
                .next()
                .await
                .map(|result| result.map(|f| state.file_service.get_directory_path(&f))),
            ActiveRecipe::Named(name) => {
                std::pin::pin!(all.try_filter_map(|x| async move {
                    Ok(match x.file_name() {
                        Some(filename) if filename == name.as_str() => {
                            Some(state.file_service.get_directory_path(&x))
                        }
                        _ => None,
                    })
                }))
                .next()
                .await
            }
            ActiveRecipe::External(path_buf) => path_buf.exists().then(|| Ok(path_buf.clone())),
        };
        match maybe {
            Some(x) => x,
            None => Err(TransactionError::Other(anyhow::anyhow!(
                "No collection exists"
            ))),
        }
    }

    // Todo: Cache list instead of getting it in each acquisition (e.g. only get it when run is over to accomodate for new images without restart)
    async fn next_image(
        &self,
        state: &mut super::DeviceState,
    ) -> anyhow::Result<PilatusDynamicImage> {
        let next = {
            let mut lock = self.pending_active.lock().await;
            if lock.is_empty() {
                *lock = pilatus::visit_directory_files(self.get_collection_directory(state).await?)
                    .filter_map(|x| async {
                        let entry = x.ok()?;

                        (entry
                            .file_name()
                            .to_str()?
                            .ends_with(&self.params.file_ending))
                        .then_some(ExistingCollectionEntry(entry.path()))
                    })
                    .collect::<BinaryHeap<_>>()
                    .await;
            }
            if state.paused {
                lock.peek().cloned()
            } else {
                lock.pop()
            }
        };

        let Some(path) = next else {
            return Err(anyhow::anyhow!("Stop streaming, there is no file"));
        };

        let image_data = tokio::fs::read(&path.0).await?;
        let img =
            tokio::task::spawn_blocking(move || image::load_from_memory(&image_data)).await??;
        debug!(
            "Publish '{:?}'",
            &path.0.file_name().and_then(|x| x.to_str()).unwrap_or("")
        );
        Ok(img.try_into()?)
    }
}

#[derive(PartialEq, Eq, Clone)]
pub(super) struct ExistingCollectionEntry(PathBuf);

impl PartialOrd for ExistingCollectionEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ExistingCollectionEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.0.cmp(&self.0)
    }
}
