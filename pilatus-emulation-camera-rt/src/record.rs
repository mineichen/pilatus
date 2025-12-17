use std::{num::NonZeroU32, sync::Arc, time::SystemTime};

use super::DeviceState;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::{StreamExt, TryStreamExt};
use minfac::ServiceCollection;
use pilatus::{
    FileService, Name, RelativeFilePath,
    device::{ActorErrorResultExtensions, ActorSystem, DeviceId, HandlerResult, Step2},
};
use pilatus_axum::{
    ServiceCollectionExtensions,
    extract::{InjectRegistered, Json, Path},
    http::StatusCode,
};
use pilatus_engineering::image::{
    ImageEncoderTrait, ImageKey, ImageWithMeta, SubscribeDynamicImageMessage,
};
use pilatus_engineering::{camera::RecordMessage, image::ImageEncoder};
use serde::Deserialize;
use tracing::debug;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.register_web("engineering/emulation-camera", |r| {
        r.http("/{device_id}/record/{collection_name}", |f| {
            f.put(record_web)
        })
    })
}

impl DeviceState {
    pub(super) async fn record(
        &mut self,
        msg: RecordMessage,
        reg: futures::stream::AbortRegistration,
    ) -> impl HandlerResult<RecordMessage> {
        let images = self
            .actor_system
            .ask(msg.source_id, SubscribeDynamicImageMessage::default())
            .await
            .map_actor_error(|_| anyhow::anyhow!("unknown error"));
        let file_service = self.file_service.clone();
        let encoder = self.encoder.clone();
        Step2(async move {
            let encoded_stream = images?
                .map(|x| async {
                    let time = std::time::SystemTime::now();
                    let encoded = encode_all(x?, encoder.clone()).await?;
                    anyhow::Ok((time, encoded))
                })
                .buffer_unordered(8);
            let abortable_stream = futures::stream::Abortable::new(encoded_stream, reg);

            let mut size_budget = msg.max_size_mb.get() as u64 * 1_000_000;

            let collection_dir = std::path::Path::new(msg.collection_name.as_str());
            abortable_stream
                .take_while(|x| {
                    let Ok((_time, images)) = x else {
                        return std::future::ready(true);
                    };
                    let required_size: usize =
                        images.iter().map(|(_, encoded)| encoded.len()).sum();

                    std::future::ready(
                        if let Some(remainer) = size_budget.checked_sub(required_size as u64) {
                            size_budget = remainer;
                            true
                        } else {
                            false
                        },
                    )
                })
                .try_for_each(|x| save_encoded(x, file_service.clone(), collection_dir))
                .await?;

            Ok(())
        })
    }
}

pub(crate) async fn encode_all(
    all: ImageWithMeta<pilatus_engineering::image::DynamicImage>,
    encoder: ImageEncoder,
) -> anyhow::Result<Vec<(ImageKey, Bytes)>> {
    debug!("Before encode");
    let r = futures::stream::iter(all.into_iter())
        .then(|(key, img)| async {
            let encoder = encoder.clone();
            tokio::task::spawn_blocking(move || {
                let i = encoder.encode(img)?;
                anyhow::Ok((key, i))
            })
            .await?
        })
        .try_collect::<Vec<_>>()
        .await;
    debug!("Finished encoding {:?}", r.as_ref().map(|x| x.len()));
    r
}

pub async fn save_encoded(
    (time, images): (SystemTime, Vec<(ImageKey, Bytes)>),
    file_service: Arc<FileService>,
    collection_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let chrono_time = DateTime::<Utc>::from(time);

    let relative_dir = collection_dir
        .join(chrono_time.format("%Y-%m-%d").to_string())
        .join(chrono_time.format("%H-%M").to_string());

    for (key, encoded) in images {
        let path = RelativeFilePath::new(relative_dir.join(format!(
            "{}_{}.png",
            chrono_time.format("%Y-%m-%d_%H-%M-%S-%3f"),
            match key.specific() {
                Some(x) => x.as_str(),
                None => "main",
            }
        )))
        .expect("String contains no invalid chars");

        file_service.add_file_unchecked(&path, &encoded).await?;
    }
    Ok(())
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
    if let Some(x) = max_size_mb
        && x.get() > 100_000
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "max_size_mb mut be <= 100_000".into(),
        ));
    }

    let msg = RecordMessage::with_max_size(
        source_id,
        collection_name,
        max_size_mb.unwrap_or(const { NonZeroU32::new(100).unwrap() }),
    );

    actor_system
        .ask(device_id, msg)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(())
}
