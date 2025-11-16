use std::{
    collections::BinaryHeap,
    ops::DerefMut,
    path::PathBuf,
    sync::{Arc, Weak},
    time::Duration,
};

use futures::{StreamExt, TryStreamExt};
use pilatus::{
    device::{ActorMessage, HandlerResult, Step2, WeakUntypedActorMessageSender},
    RelativeDirectoryPath, TransactionError,
};
use pilatus_engineering::image::{DynamicImage as PilatusDynamicImage, ImageWithMeta};
use tracing::{debug, warn};

use super::DeviceState;
use pilatus_emulation_camera::{ActiveRecipe, Params};

pub(super) struct PublishImageMessage(Weak<PublisherState>);

impl ActorMessage for PublishImageMessage {
    type Output = ();
    type Error = ();
}

impl PublishImageMessage {
    pub fn new(state: &Arc<PublisherState>) -> Self {
        Self(Arc::downgrade(state))
    }
}

impl DeviceState {
    pub(super) async fn publish_frame(
        &mut self,
        msg: PublishImageMessage,
    ) -> impl HandlerResult<PublishImageMessage> {
        let re_schedule = if let Some(strong) = msg.0.upgrade() {
            match strong.next_image(self).await {
                Ok(Some(image)) => self
                    .stream
                    .send(Ok(ImageWithMeta::with_hash(image, None)))
                    .ok()
                    .map(|_| msg.0),
                Ok(None) => {
                    debug!("Stop acquisition");
                    Some(msg.0)
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

type PendingActiveLock = tokio::sync::Mutex<(
    BinaryHeap<ExistingCollectionEntry>,
    Option<ExistingCollectionEntry>,
)>;

pub(super) struct PublisherState {
    pub params: Params,
    self_sender: WeakUntypedActorMessageSender,
    pending_active: PendingActiveLock,
}

impl PublisherState {
    pub fn new(self_sender: WeakUntypedActorMessageSender, params: Params) -> Self {
        Self {
            self_sender,
            params,
            pending_active: Default::default(),
        }
    }

    pub fn with_params(&self, params: Params) -> Self {
        Self::new(self.self_sender.clone(), params)
    }

    pub async fn enqueue(self: &Arc<Self>, state: &DeviceState) -> anyhow::Result<()> {
        let mut lock = self.pending_active.lock().await;
        if lock.0.is_empty() {
            lock.0 = self.load_collection(state).await?;
        } else {
            debug!("Items are still available. Use them");
        }

        self.self_sender
            .clone()
            .tell(PublishImageMessage::new(self))?;
        Ok(())
    }

    pub async fn send_delayed(weak: Weak<Self>) {
        if let Some(state) = weak.upgrade() {
            tokio::time::sleep(Duration::from_millis(state.params.file.interval)).await;
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
        let maybe = match &self.params.file.active {
            ActiveRecipe::Undefined => all
                .next()
                .await
                .map(|result| result.map(|f| state.file_service.get_directory_path(&f))),
            ActiveRecipe::Named(name) => {
                std::pin::pin!(all.try_filter_map(|x| async move {
                    Ok(x.file_name()
                        .filter(|filename| *filename == name.as_str())
                        .map(|_| state.file_service.get_directory_path(&x)))
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

    async fn next_image(
        &self,
        state: &mut super::DeviceState,
    ) -> anyhow::Result<Option<PilatusDynamicImage>> {
        let next = {
            let mut lock = self.pending_active.lock().await;
            let (lock, last) = lock.deref_mut();
            if lock.is_empty() {
                if !self.params.file.auto_restart {
                    return Ok(None);
                }
                *lock = self.load_collection(state).await?;
            }
            if !state.paused || last.is_none() {
                *last = lock.pop();
            }

            last.clone()
        };

        let Some(path) = next else {
            return Err(anyhow::anyhow!("Stop streaming, there is no file"));
        };

        let image_data = tokio::fs::read(&path.0).await?;
        let img =
            tokio::task::spawn_blocking(move || image::load_from_memory(&image_data)).await??;

        let pilatus_image: PilatusDynamicImage = img.try_into()?;
        debug!(
            "Publish '{:?}': {:?}",
            &path.0.file_name().and_then(|x| x.to_str()).unwrap_or(""),
            &pilatus_image
        );
        Ok(Some(pilatus_image))
    }

    async fn load_collection(
        &self,
        state: &DeviceState,
    ) -> Result<BinaryHeap<ExistingCollectionEntry>, anyhow::Error> {
        Ok(
            pilatus::visit_directory_files(self.get_collection_directory(state).await?)
                .filter_map(|x| async {
                    let entry = x.ok()?;

                    (entry
                        .file_name()
                        .to_str()?
                        .ends_with(&self.params.file.file_ending))
                    .then_some(ExistingCollectionEntry(entry.path()))
                })
                .collect::<BinaryHeap<_>>()
                .await,
        )
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
