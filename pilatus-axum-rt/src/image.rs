use std::time::SystemTime;

use axum::{extract::Query, response::sse::Event};
use futures::{Stream, StreamExt};
use image::{ImageEncoder, ImageResult};
use minfac::ServiceCollection;
use pilatus::device::{ActorSystem, DeviceId};
use pilatus_axum::{
    extract::{ws::WebSocketUpgrade, InjectRegistered, Json, Path},
    http::StatusCode,
    image::{DefaultImageStreamer, LocalizableImageStreamer},
    sse::Sse,
    AppendHeaders, IntoResponse, ServiceCollectionExtensions,
};
use pilatus_engineering::image::{
    GetImageMessage, LumaImage, SubscribeImageMessage, SubscribeLocalizableImageMessage,
};
use tracing::{debug, warn};

pub(super) fn register_services(c: &mut ServiceCollection) {
    #[rustfmt::skip]
    c.register_web("image", |x| x
        .http("/list/stream", |m| m.get(list_stream_devices))
        .http("/list/stream/localizable", |m| m.get(list_localizable_stream_devices))
        .http("/stream", |m| m.get(stream_image_handler))
        .http("/stream/localizable", |m| m.get(stream_localizable_image_handler))
        .http("/:device_id/single", |m| m.get(single_image_handler))
        .http("/:device_id/frame_intervals", |m| m.get(stream_frame_interval))
    );
}

async fn stream_frame_interval(
    Path(device_id): Path<DeviceId>,
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
) -> Result<Sse<impl Stream<Item = Result<Event, anyhow::Error>>>, StatusCode> {
    let sender = actor_system
        .ask(device_id, SubscribeImageMessage::default())
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let mut last_timestamp: Option<SystemTime> = None;

    Ok(Sse::new(sender.filter_map(move |_| {
        let time = std::time::SystemTime::now();
        let data = last_timestamp
            .and_then(|last| time.duration_since(last).ok())
            .map(|t| Ok(Event::default().data(t.as_millis().to_string())));

        last_timestamp = Some(time);

        async move { data }
    })))
}

async fn single_image_handler(
    Path(device_id): Path<DeviceId>,
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
) -> Result<impl IntoResponse, StatusCode> {
    let img = LumaImage::from(
        actor_system
            .ask(device_id, GetImageMessage::default())
            .await
            .map_err(|_| StatusCode::BAD_REQUEST)?,
    );
    pilatus::execute_blocking(move || {
        let dims = img.dimensions();
        let mut buf = Vec::with_capacity(dims.0.get() as usize * dims.1.get() as usize / 4);
        let codec = image::codecs::png::PngEncoder::new(&mut buf);
        codec.write_image(
            img.buffer(),
            dims.0.get(),
            dims.1.get(),
            image::ColorType::L8,
        )?;
        let name = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S");
        ImageResult::Ok((
            AppendHeaders([(
                "Content-Disposition",
                format!("attachment; filename=\"{name}.png\""),
            )]),
            buf,
        ))
    })
    .await
    .map_err(|_| StatusCode::BAD_REQUEST)
}

async fn list_stream_devices(
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
) -> impl IntoResponse {
    Json(actor_system.list_devices_for_message_type::<SubscribeImageMessage>())
}

async fn list_localizable_stream_devices(
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
) -> impl IntoResponse {
    Json(actor_system.list_devices_for_message_type::<SubscribeLocalizableImageMessage>())
}

async fn stream_image_handler(
    upgrade: WebSocketUpgrade,
    Query(StreamQuery { device_id }): Query<StreamQuery>,
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    debug!("Start streaming images: {device_id:?}");
    DefaultImageStreamer::stream_image(upgrade, device_id, actor_system, |x| async { Ok(x.image) })
        .await
        .map_err(|e| {
            warn!("Couldn't establish connection: {e:?}");
            e
        })
}

async fn stream_localizable_image_handler(
    upgrade: WebSocketUpgrade,
    Query(StreamQuery { device_id }): Query<StreamQuery>,
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    debug!("Start streaming images: {device_id:?}");
    LocalizableImageStreamer::stream_image(upgrade, device_id, actor_system, |x| async {
        Ok(x.image)
    })
    .await
    .map_err(|e| {
        warn!("Couldn't establish connection: {e:?}");
        e
    })
}

#[derive(serde::Deserialize)]
struct StreamQuery {
    device_id: Option<DeviceId>,
}
