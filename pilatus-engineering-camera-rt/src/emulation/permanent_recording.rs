use std::{future::Future, time::Duration};

use futures::{
    channel::mpsc::{self, Sender},
    future::Either,
    StreamExt,
};
use pilatus::{
    device::{ActorError, DeviceId, WeakUntypedActorMessageSender},
    Name,
};
use serde::{Deserialize, Serialize};
use tracing::warn;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PermanentRecordingConfig {
    collection_name: Name,
    source_id: DeviceId,
}

pub(super) fn setup_permanent_recording(
    weak_self_sender: WeakUntypedActorMessageSender,
    params: &Option<PermanentRecordingConfig>,
) -> (
    Sender<Option<PermanentRecordingConfig>>,
    impl Future<Output = ()>,
) {
    let (mut recording_sender, recording_receiver) = mpsc::channel(2);
    recording_sender
        .try_send(params.clone())
        .expect("Just created above with capacity 2");
    (
        recording_sender,
        handle_permanent_recording(weak_self_sender, recording_receiver),
    )
}

async fn handle_permanent_recording(
    mut self_sender: WeakUntypedActorMessageSender,
    mut recv: mpsc::Receiver<Option<PermanentRecordingConfig>>,
) {
    while let Some(mut maybe_next) = recv.next().await {
        loop {
            let Some(config) = maybe_next.take() else {
                break;
            };
            let record_task = std::pin::pin!(self_sender.ask(
                pilatus_engineering_camera::RecordMessage::with_max_size(
                    config.source_id.clone(),
                    config.collection_name.clone(),
                    4_000_000.try_into().unwrap(),
                ),
            ));
            match futures::future::select(record_task, &mut recv.next()).await {
                Either::Left((Ok(_) | Err(ActorError::UnknownDevice(..)), _)) => break,
                Either::Left((Err(e), _)) => {
                    warn!("Error during record: {e:?}. Try again in 1s");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    maybe_next = Some(config);
                    continue;
                }
                Either::Right((Some(next_job), _)) => {
                    maybe_next = next_job;
                    continue;
                }
                Either::Right((None, _)) => return,
            }
        }
    }
}
