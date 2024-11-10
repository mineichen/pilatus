use std::{num::NonZeroU32, time::Duration};

use chrono::{DateTime, Utc};
use futures::StreamExt;
use minfac::ServiceCollection;
use pilatus::{
    device::{
        ActorError, ActorErrorResultExtensions, ActorMessage, ActorResult, ActorSystem, DeviceId,
    },
    Name, RelativeFilePath,
};
use pilatus_axum::{
    extract::{InjectRegistered, Json, Path},
    http::StatusCode,
    ServiceCollectionExtensions,
};
use pilatus_engineering::image::{StreamImageError, SubscribeDynamicImageMessage};
use serde::Deserialize;

use super::DeviceState;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.register_web("engineering/emulation-camera", |r| {
        r.http("/:device_id/record/:collection_name", |f| f.put(record_web))
    })
}

#[derive(Debug)]
pub struct RecordMessage {
    source_id: DeviceId,
    collection_name: pilatus::Name,
    max_size_mb: Option<NonZeroU32>,
}

impl RecordMessage {
    fn new(
        source_id: DeviceId,
        collection_name: pilatus::Name,
        max_size_mb: Option<NonZeroU32>,
    ) -> anyhow::Result<Self> {
        match max_size_mb.map(NonZeroU32::get) {
            Some(100_001..) => Err(anyhow::anyhow!("max_size_mb > 100_000")),
            _ => Ok(Self {
                source_id,
                collection_name,
                max_size_mb,
            }),
        }
    }
    pub fn with_max_size(
        source_id: DeviceId,
        collection_name: pilatus::Name,
        max_size_mb: NonZeroU32,
    ) -> Self {
        Self {
            source_id,
            collection_name,
            max_size_mb: Some(max_size_mb),
        }
    }
}

impl ActorMessage for RecordMessage {
    type Output = ();
    type Error = anyhow::Error;
}

impl DeviceState {
    pub(super) async fn record(
        &mut self,
        msg: RecordMessage,
        reg: futures::stream::AbortRegistration,
    ) -> ActorResult<RecordMessage> {
        let images = self
            .actor_system
            .ask(msg.source_id, SubscribeDynamicImageMessage::default())
            .await
            .map_actor_error(|_| anyhow::anyhow!("unknown error"))?;

        let encoded_stream = images
            .filter(|e| std::future::ready(!matches!(e, Err(StreamImageError::MissedItems(..)))))
            .map(|x| async move {
                let data = x?;
                tokio::task::spawn_blocking(move || {
                    anyhow::Ok((
                        // todo: Take from metadata when it is available
                        std::time::SystemTime::now(),
                        data.image.encode_png()?,
                    ))
                })
                .await?
            })
            .buffer_unordered(8);
        let mut abortable_stream = futures::stream::Abortable::new(encoded_stream, reg);

        let mut size_budget =
            msg.max_size_mb.map(NonZeroU32::get).unwrap_or(100) as u64 * 1_000_000;

        let collection_dir = std::path::Path::new(msg.collection_name.as_str());
        while let Some(x) =
            tokio::time::timeout(Duration::from_secs(5), abortable_stream.next()).await?
        {
            let (time, encoded) = x?;
            let Some(remainer) = size_budget.checked_sub(encoded.len() as u64) else {
                break;
            };
            let chrono_time = DateTime::<Utc>::from(time);
            size_budget = remainer;

            let relative_dir = collection_dir
                .join(&chrono_time.format("%Y-%m-%d").to_string())
                .join(&chrono_time.format("%H-%M").to_string());

            tokio::fs::create_dir_all(&relative_dir)
                .await
                .map_err(ActorError::custom)?;

            let path = RelativeFilePath::new(relative_dir.join(format!(
                "{}.png",
                chrono_time.format("%Y-%m-%d_%H-%M-%S-%3f")
            )))
            .expect("String contains no invalid chars");

            self.file_service
                .add_file_unchecked(&path, &encoded)
                .await?;
        }

        Ok(())
    }
}

#[derive(Deserialize)]
struct RecordBody {
    source_id: DeviceId,
    max_size_mb: Option<NonZeroU32>,
}
#[derive(Deserialize)]
struct RecordPath {
    collection_name: Name,
    device_id: DeviceId,
}

async fn record_web(
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
    Path(RecordPath {
        device_id,
        collection_name,
    }): Path<RecordPath>,
    Json(RecordBody {
        source_id,
        max_size_mb,
    }): Json<RecordBody>,
) -> Result<(), (StatusCode, String)> {
    let msg = RecordMessage::new(source_id, collection_name, max_size_mb)
        .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;

    actor_system
        .ask(device_id, msg)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(())
}
