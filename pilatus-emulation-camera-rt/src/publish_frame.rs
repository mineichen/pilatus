use std::{
    collections::BinaryHeap,
    ops::DerefMut,
    path::PathBuf,
    sync::{Arc, Weak},
    time::Duration,
};

use futures::{StreamExt, TryStreamExt};
use imbuf_image::ImageIntoImbuf;
use pilatus::{
    FileService, RelativeDirectoryPath,
    device::{ActorMessage, ActorResult, WeakUntypedActorMessageSender},
};
use pilatus_engineering::image::{DynamicImage as ImbufDynamicImage, ImageWithMeta};
use tokio::io;
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
    ) -> ActorResult<PublishImageMessage> {
        let move_to_next = !self.paused;
        let re_schedule = match PublisherState::next_image_if_upgradeable(
            &msg.0,
            &self.file_service,
            move_to_next,
        )
        .await
        {
            Ok(Some((image, path))) => {
                debug!(
                    "Publish '{:?}' to {} receivers: {:?}",
                    &path.0.file_name().and_then(|x| x.to_str()).unwrap_or(""),
                    self.stream.receiver_count(),
                    &image
                );
                let image = ImageWithMeta::with_hash(image, None);
                self.stream.send(Ok(image)).ok().map(|_| msg.0)
            }
            Ok(None) => {
                debug!("Stop acquisition");
                None
            }
            Err(e) => {
                warn!("Stop due to acquisition error: {e:?}");
                let error = pilatus_engineering::image::StreamImageError::Acquisition {
                    error: Arc::new(e),
                };
                let _ = self.stream.send(Err(error)).inspect_err(|e| {
                    warn!("Couldn't send error to any subscriber: {e}");
                });
                None
            }
        };

        if let Some(weak) = re_schedule {
            PublisherState::schedule_next(weak);
        }
        Ok(())
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
            pending_active: tokio::sync::Mutex::default(),
        }
    }

    pub fn with_params(&self, params: Params) -> Self {
        Self::new(self.self_sender.clone(), params)
    }

    pub async fn enqueue(self: &Arc<Self>, state: &DeviceState) -> anyhow::Result<()> {
        let mut lock = self.pending_active.lock().await;
        if lock.0.is_empty() {
            lock.0 = self.load_collection(&state.file_service).await?;
        } else {
            debug!("Items are still available. Use them");
        }

        self.self_sender
            .clone()
            .tell(PublishImageMessage::new(self))?;
        Ok(())
    }

    /// Sends the next [`PublishImageMessage`] after the configured interval.
    ///
    /// Runs detached and holds only weak references, so it never delays the shutdown of the device
    /// (pending actor tasks are awaited when a device is stopped).
    pub fn schedule_next(weak: Weak<Self>) {
        let Some(state) = weak.upgrade() else {
            return;
        };
        let interval = Duration::from_millis(state.params.file.interval);
        let mut sender = state.self_sender.clone();
        drop(state);
        tokio::spawn(async move {
            tokio::time::sleep(interval).await;
            sender.tell(PublishImageMessage(weak)).ok();
        });
    }

    pub async fn get_collection_directory(&self, state: &FileService) -> io::Result<PathBuf> {
        let mut all = state.stream_directories(RelativeDirectoryPath::root());
        match &self.params.file.active {
            ActiveRecipe::Undefined => all
                .next()
                .await
                .map(|result| result.map(|f| state.get_directory_path(&f))),
            ActiveRecipe::Named(name) => {
                std::pin::pin!(all.try_filter_map(|x| async move {
                    Ok(x.file_name()
                        .filter(|filename| *filename == name.as_str())
                        .map(|_| state.get_directory_path(&x)))
                }))
                .next()
                .await
            }
            ActiveRecipe::External(path_buf) => path_buf.exists().then(|| Ok(path_buf.clone())),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No collection exists"))?
    }

    pub(crate) async fn next_image_if_upgradeable(
        weak: &Weak<Self>,
        state: &FileService,
        move_to_next: bool,
    ) -> anyhow::Result<Option<(ImbufDynamicImage, ExistingCollectionEntry)>> {
        let Some(this) = weak.upgrade() else {
            return Ok(None);
        };
        this.next_image(state, move_to_next).await
    }
    pub(crate) async fn next_image(
        &self,
        state: &FileService,
        move_to_next: bool,
    ) -> anyhow::Result<Option<(ImbufDynamicImage, ExistingCollectionEntry)>> {
        let next = {
            let mut lock = self.pending_active.lock().await;
            let (heap, last) = lock.deref_mut();
            if heap.is_empty() {
                if !self.params.file.auto_restart {
                    return Ok(None);
                }
                *heap = self.load_collection(state).await?;
            }
            if move_to_next || last.is_none() {
                *last = heap.pop();
            }

            last.clone()
        };

        let Some(path) = next else {
            return Err(anyhow::anyhow!("Stop streaming, there is no file"));
        };

        let image_data = tokio::fs::read(&path.0).await?;
        let img =
            tokio::task::spawn_blocking(move || image::load_from_memory(&image_data)).await??;

        let pilatus_image = img.into_imbuf_dynamic()?;
        Ok(Some((pilatus_image, path)))
    }

    async fn load_collection(
        &self,
        state: &FileService,
    ) -> Result<BinaryHeap<ExistingCollectionEntry>, anyhow::Error> {
        Ok(
            pilatus::visit_directory_files(self.get_collection_directory(&state).await?)
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
