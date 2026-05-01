use std::{convert::Infallible, time::SystemTime};

use anyhow::anyhow;
use axum::{extract::Query, response::sse::Event};
use futures::{stream::BoxStream, Stream, StreamExt};
use minfac::ServiceCollection;
use pilatus::device::{
    ActorError, ActorErrorResultExtensions, ActorSystem, DeviceId, DynamicIdentifier,
};
use pilatus_axum::{
    extract::{ws::WebSocketUpgrade, InjectRegistered, Json, Path},
    sse::Sse,
    AppendHeaders, DeviceJsonError, Html, IntoResponse, ServiceCollectionExtensions,
};
use pilatus_engineering::image::{
    DefaultImageStreamer, ImageStreamer, LocalizableImageStreamer, MetaImageEncodeTask,
    MetaImageEncoder, StreamingImageFormat,
};
use pilatus_engineering::image::{
    DynamicImage, GetImageMessage, ImageEncoder, ImageEncoderTrait, ImageWithMeta, LumaImage,
    StreamImageError, SubscribeDynamicImageMessage, SubscribeImageMessage,
    SubscribeLocalizableImageMessage,
};
use tracing::{debug, warn};

pub(super) fn register_services(c: &mut ServiceCollection) {
    // Note: Web routes that use WebSocketUpgrade require WebSocketDropperService
    // to be registered (typically by pilatus-axum-rt). If pilatus-engineering-rt
    // is used without pilatus-axum-rt, the dependency checker will fail at build time.
    // This is expected behavior - web routes should only be used with pilatus-axum-rt.
    #[rustfmt::skip]
    c.register_web("image", |x| x
        .http("", |m| m.get(single_dynamic_image_handler))
        .http("/list/subscribe", |m| m.get(list_subscribe_devices))
        .http("/list/stream", |m| m.get(list_stream_devices))
        .http("/list/stream/localizable", |m| m.get(list_localizable_stream_devices))
        .http("/stream", |m| m.get(stream_image_handler))
        .http("/subscribe", |m| m.get(subscribe_image_handler))
        .http("/stream/localizable", |m| m.get(stream_localizable_image_handler))
        .http("/viewer", |m| m.get(image_viewer))
        .http("/{device_id}/single", |m| m.get(single_luma_image_handler))
        .http("/{device_id}/frame_intervals", |m| m.get(stream_frame_interval))
    );
}

async fn stream_frame_interval(
    Path(device_id): Path<DeviceId>,
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
) -> Result<Sse<impl Stream<Item = Result<Event, anyhow::Error>>>, DeviceJsonError<anyhow::Error>> {
    let sender = actor_system
        .ask(device_id, SubscribeImageMessage::default())
        .await?;

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

async fn single_luma_image_handler(
    Path(device_id): Path<DeviceId>,
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
) -> Result<impl IntoResponse, DeviceJsonError<anyhow::Error>> {
    let img = LumaImage::from(
        actor_system
            .ask(device_id, GetImageMessage::default())
            .await?,
    );
    Ok(pilatus::execute_blocking(move || {
        let dims = img.dimensions();
        let mut buf = Vec::with_capacity(dims.0.get() as usize * dims.1.get() as usize / 4);
        let codec = image::codecs::png::PngEncoder::new(&mut buf);

        image::ImageEncoder::write_image(
            codec,
            img.buffer(),
            dims.0.get(),
            dims.1.get(),
            image::ExtendedColorType::L8,
        )?;
        let name = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S");
        anyhow::Ok((
            AppendHeaders([(
                "Content-Disposition",
                format!("attachment; filename=\"{name}.png\""),
            )]),
            buf,
        ))
    })
    .await
    .map_err(ActorError::custom)?)
}

async fn single_dynamic_image_handler(
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
    InjectRegistered(image_encoder): InjectRegistered<ImageEncoder>,
    Query(id): Query<DynamicIdentifier>,
) -> Result<impl IntoResponse, DeviceJsonError<anyhow::Error>> {
    let img = actor_system
        .ask(id, SubscribeDynamicImageMessage::default())
        .await
        .map_actor_error(|_: Infallible| unreachable!())?
        .next()
        .await
        .ok_or_else(|| ActorError::Custom(anyhow!("No images in stream")))?
        .map_err(ActorError::custom)?
        .image;
    Ok(pilatus::execute_blocking(move || {
        let buf = image_encoder.encode(img)?;
        let name = chrono::Utc::now().format("%Y-%m-%d_%H-%M-%S-%f");
        anyhow::Ok((
            AppendHeaders([(
                "Content-Disposition",
                format!("attachment; filename=\"{name}.png\""),
            )]),
            buf,
        ))
    })
    .await
    .map_err(ActorError::custom)?)
}

#[cfg(debug_assertions)]
async fn image_viewer() -> Result<Html<String>, (pilatus_axum::http::StatusCode, String)> {
    tokio::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("image_viewer.html"),
    )
    .await
    .map(Into::into)
    .map_err(|e| {
        (
            pilatus_axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            e.to_string(),
        )
    })
}

#[cfg(not(debug_assertions))]
async fn image_viewer() -> Html<&'static str> {
    include_str!("../../resources/image_viewer.html").into()
}

async fn list_subscribe_devices(
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
) -> impl IntoResponse {
    Json(actor_system.list_devices_for_message_type::<SubscribeDynamicImageMessage>())
}

async fn list_stream_devices(
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
) -> impl IntoResponse {
    warn!("Deprecated: Use /list/subscribe instead, which can handle Images with various channes and MetaData");
    Json(actor_system.list_devices_for_message_type::<SubscribeImageMessage>())
}

async fn list_localizable_stream_devices(
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
) -> impl IntoResponse {
    warn!("Deprecated: Use /list/subscribe instead, which can handle Images with various channes and MetaData");
    Json(actor_system.list_devices_for_message_type::<SubscribeLocalizableImageMessage>())
}

async fn subscribe_image_handler(
    upgrade: WebSocketUpgrade,
    Query(StreamQuery { format }): Query<StreamQuery>,
    Query(device_identifier): Query<DynamicIdentifier>,
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
    InjectRegistered(encoder): InjectRegistered<MetaImageEncoder>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    debug!("Start streaming websocket images: {device_identifier:?}");

    ImageStreamer::<SubscribeDynamicImageMessage, BoxStream<'static, _>, _>::stream_image(
        upgrade,
        device_identifier,
        actor_system,
        move |x: Result<ImageWithMeta<DynamicImage>, StreamImageError<DynamicImage>>| async move {
            Ok(MetaImageEncodeTask {
                image: x,
                format,
            })
        },
        encoder,
    )
    .await
    .map_err(|e| {
        warn!("Couldn't establish connection: {e:?}");
        e
    })
}

async fn stream_image_handler(
    upgrade: WebSocketUpgrade,
    Query(device_identifier): Query<DynamicIdentifier>,
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
    InjectRegistered(encoder): InjectRegistered<MetaImageEncoder>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    debug!("Start streaming images: {device_identifier:?}");
    DefaultImageStreamer::stream_image(upgrade, device_identifier, actor_system, |x| async {
        Ok(x.image)
    }, encoder)
    .await
    .map_err(|e| {
        warn!("Couldn't establish connection: {e:?}");
        e
    })
}

async fn stream_localizable_image_handler(
    upgrade: WebSocketUpgrade,
    Query(device_identifier): Query<DynamicIdentifier>,
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
    InjectRegistered(encoder): InjectRegistered<MetaImageEncoder>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    debug!("Start streaming images: {device_identifier:?}");
    LocalizableImageStreamer::stream_image(upgrade, device_identifier, actor_system, |x| async {
        Ok(x.image)
    }, encoder)
    .await
    .map_err(|e| {
        warn!("Couldn't establish connection: {e:?}");
        e
    })
}

#[derive(serde::Deserialize)]
struct StreamQuery {
    #[serde(default)]
    format: StreamingImageFormat,
}
