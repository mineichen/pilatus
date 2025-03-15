use std::fmt::{self, Display, Formatter, Write};
use std::io::{self, ErrorKind};

use async_zip::error::ZipError;
use futures::{SinkExt, Stream, StreamExt};
use minfac::ServiceCollection;
use pilatus::RecipeService;
use pilatus::{
    device::DeviceId, Name, ParameterUpdate, RecipeId, RecipeMetadata, TransactionError,
    TransactionOptions,
};
use pilatus_axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        InjectRegistered, Json, Path, Query,
    },
    http::StatusCode,
    IntoResponse, ServiceCollectionExtensions,
};
use sealedstruct::ValidationErrors;
use tracing::debug;
use uuid::Uuid;

mod export;
mod file;
mod import;

pub(super) fn register_services(c: &mut ServiceCollection) {
    #[rustfmt::skip]
    c.register_web("recipe", |r| r
        .http("/get_all", |m| m.get(get_all))
        .http("/new_default", |m| m.put(add_default_recipe))
        .http("/stream",|m| m.get(stream_recipe_update_handler))
        .http("/commit", |m| m.put(commit_active))
        .http("/restore", |m| m.put(restore_active))
        .http("/{id}/meta", |m| m.put(update_recipe_metadata))
        .http("/{id}/clone", |m| m.put(clone_recipe))
        .http("/{id}", |m| m.delete(delete_recipe))
        .http("/{id}/device/{device_id}/params", |m| m.put(update_device_params))
        .http("/{id}/device/{device_id}/name", |m| m.put(update_device_name))
        .http("/{id}/device/{device_id}/committed", |m| m.put(restore_committed))
    );

    file::register_services(c);
    export::register_services(c);
    import::register_services(c);
}

pub fn zip_to_io_error(e: ZipError) -> io::Error {
    io::Error::new(
        match e {
            ZipError::FeatureNotSupported(_) => ErrorKind::Unsupported,
            ZipError::CompressionNotSupported(_) => ErrorKind::Unsupported,
            ZipError::AttributeCompatibilityNotSupported(_) => ErrorKind::Unsupported,
            ZipError::TargetZip64NotSupported => ErrorKind::Unsupported,
            ZipError::ExtraFieldTooLarge => ErrorKind::InvalidData,
            ZipError::CommentTooLarge => ErrorKind::InvalidData,
            ZipError::FileNameTooLarge => ErrorKind::InvalidData,
            ZipError::UnableToLocateEOCDR => ErrorKind::InvalidData,
            ZipError::InvalidExtraFieldHeader(_, _) => ErrorKind::InvalidData,
            ZipError::Zip64ExtendedFieldIncomplete => ErrorKind::InvalidData,
            ZipError::UnexpectedHeaderError(_, _) => ErrorKind::InvalidData,
            ZipError::CRC32CheckError => ErrorKind::InvalidData,
            ZipError::Zip64Needed(_) => ErrorKind::InvalidData,
            ZipError::EntryIndexOutOfBounds => ErrorKind::InvalidData,
            ZipError::EOFNotReached => ErrorKind::InvalidData,
            ZipError::UpstreamReadError(_) => ErrorKind::BrokenPipe,
            _ => ErrorKind::Other,
        },
        e,
    )
}

async fn stream_recipe_update_handler(
    upgrade: WebSocketUpgrade,
    InjectRegistered(service): InjectRegistered<RecipeService>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let watcher = service.get_update_receiver();

    Ok(upgrade.into_inner().on_upgrade(move |socket| async move {
        debug!("Subscribe recipe update broadcast");
        handle_socket(socket, watcher).await;
        debug!("Recipe update subscription ended.");
    }))
}

async fn handle_socket(socket: WebSocket, watcher: impl Stream<Item = Uuid>) {
    let (mut socket_tx, mut socket_rx) = socket.split();
    futures::pin_mut!(watcher);
    {
        tokio::select!(
            _ = async {
                while let Some(data) = watcher.next().await {
                    if socket_tx
                        .send(Message::Text(data.to_string().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            } => {},
            _ = async {
                while let Some(r) = socket_rx.next().await {
                    if r.is_err() {
                        break;
                    }
                }
            } => {}
        );
    };
    let _ignore_if_not_closeable = socket_rx
        .reunite(socket_tx)
        .expect("Guaranted to be same source")
        .close()
        .await;
}

async fn delete_recipe(
    InjectRegistered(service): InjectRegistered<RecipeService>,
    Path(recipe_id): Path<RecipeId>,
    Query(options): Query<TransactionOptions>,
) -> Result<(), (StatusCode, String)> {
    service
        .delete_recipe_with(recipe_id, options)
        .await
        .map_err(transaction_error_to_http_resonse)
}

async fn clone_recipe(
    InjectRegistered(service): InjectRegistered<RecipeService>,
    Path(recipe_id): Path<RecipeId>,
    Query(options): Query<TransactionOptions>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let recipe = service
        .duplicate_recipe_with(recipe_id, options)
        .await
        .map_err(transaction_error_to_http_resonse)?;
    Ok(Json(recipe))
}

async fn add_default_recipe(
    InjectRegistered(service): InjectRegistered<RecipeService>,
    Query(options): Query<TransactionOptions>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let recipe = service
        .add_new_default_recipe_with(options)
        .await
        .map_err(transaction_error_to_http_resonse)?;
    Ok(Json(recipe))
}

async fn get_all(InjectRegistered(service): InjectRegistered<RecipeService>) -> impl IntoResponse {
    let recipes = service.state().await;
    Json(recipes)
}

async fn update_device_params(
    InjectRegistered(service): InjectRegistered<RecipeService>,
    Path((recipe_id, device_id)): Path<(RecipeId, DeviceId)>,
    Query(options): Query<TransactionOptions>,
    Json(param_update): Json<ParameterUpdate>,
) -> Result<(), (StatusCode, String)> {
    service
        .update_device_params_with(recipe_id, device_id, param_update, options)
        .await
        .map_err(|e| {
            struct DeviceConfigWrapper(ValidationErrors);
            impl Display for DeviceConfigWrapper {
                fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                    for x in self.0.iter() {
                        f.write_str(&x.reason)?;
                        f.write_char('\n')?;
                    }
                    Ok(())
                }
            }
            (
                StatusCode::BAD_REQUEST,
                match e {
                    TransactionError::InvalidDeviceConfig(e) => DeviceConfigWrapper(e).to_string(),
                    _ => e.to_string(),
                },
            )
        })
}

async fn update_recipe_metadata(
    InjectRegistered(service): InjectRegistered<RecipeService>,
    Path(id): Path<RecipeId>,
    Query(options): Query<TransactionOptions>,
    Json(data): Json<RecipeMetadata>,
) -> Result<(), (StatusCode, String)> {
    service
        .update_recipe_metadata_with(id, data, options)
        .await
        .map_err(transaction_error_to_http_resonse)
}

#[derive(serde::Deserialize)]
struct TransactionIdWrapper {
    key: Option<Uuid>,
}

async fn commit_active(
    InjectRegistered(service): InjectRegistered<RecipeService>,
    Query(options): Query<TransactionIdWrapper>,
) -> Result<(), (StatusCode, String)> {
    service
        .commit_active_with(options.key.unwrap_or_else(Uuid::new_v4))
        .await
        .map_err(transaction_error_to_http_resonse)
}

async fn restore_active(
    InjectRegistered(service): InjectRegistered<RecipeService>,
    Query(options): Query<TransactionIdWrapper>,
) -> Result<(), (StatusCode, String)> {
    service
        .restore_active_with(options.key.unwrap_or_else(Uuid::new_v4))
        .await
        .map_err(transaction_error_to_http_resonse)
}

async fn restore_committed(
    InjectRegistered(service): InjectRegistered<RecipeService>,
    Path((recipe_id, device_id)): Path<(RecipeId, DeviceId)>,
    Query(options): Query<TransactionIdWrapper>,
) -> Result<(), (StatusCode, String)> {
    service
        .restore_committed(
            recipe_id,
            device_id,
            options.key.unwrap_or_else(Uuid::new_v4),
        )
        .await
        .map_err(transaction_error_to_http_resonse)
}

async fn update_device_name(
    InjectRegistered(service): InjectRegistered<RecipeService>,
    Path((recipe_id, device_id)): Path<(RecipeId, DeviceId)>,
    Query(options): Query<TransactionOptions>,
    device_name: String,
) -> Result<(), (StatusCode, String)> {
    let device_name =
        Name::new(device_name).map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;
    service
        .update_device_name_with(recipe_id, device_id, device_name, options)
        .await
        .map_err(transaction_error_to_http_resonse)
}

fn transaction_error_to_http_resonse(e: TransactionError) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, e.to_string())
}
