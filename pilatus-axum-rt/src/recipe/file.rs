use bytes::Bytes;
use minfac::ServiceCollection;
use pilatus::{
    device::{ActorSystem, DeviceId},
    AddFileMessage, DeleteFileMessage, GetFileMessage, ListFilesMessage, RelativeDirectoryPathBuf,
    RelativeFilePath,
};
use pilatus_axum::{
    extract::{InjectRegistered, Json, Path},
    http::StatusCode,
    IntoResponse, ServiceCollectionExtensions,
};

pub(super) fn register_services(c: &mut ServiceCollection) {
    #[rustfmt::skip]
    c.register_web("recipe/file", |r| r
        .http("/list/{device_id}/{*path}", |m| m
            .get(list_files))
        .http("/list/{device_id}", |m| m
            .get(list_files_root))
        .http("/{device_id}/{*filename}", |m| m
            .get(get_file)
            .put(add_file)
            .delete(delete_file))
    );
}

async fn get_file(
    Path((device_id, path)): Path<(DeviceId, RelativeFilePath)>,
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    actor_system
        .ask(device_id, GetFileMessage { path })
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Bummer, it failed: {e:?}")))
}

async fn delete_file(
    Path((device_id, path)): Path<(DeviceId, RelativeFilePath)>,
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    actor_system
        .ask(device_id, DeleteFileMessage { path })
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Bummer, it failed: {e:?}")))
}

async fn add_file(
    Path((device_id, path)): Path<(DeviceId, RelativeFilePath)>,
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
    data: Bytes,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    actor_system
        .ask(device_id, AddFileMessage { path, data })
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Bummer, it failed: {e:?}")))
}

async fn list_files_root(
    Path(device_id): Path<DeviceId>,
    inj: InjectRegistered<ActorSystem>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    list_files(Path((device_id, RelativeDirectoryPathBuf::root())), inj).await
}

async fn list_files(
    Path((device_id, path)): Path<(DeviceId, RelativeDirectoryPathBuf)>,
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let files = actor_system
        .ask(device_id, ListFilesMessage { path })
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Bummer, it failed: {e:?}")))?;

    Ok(Json(files))
}
