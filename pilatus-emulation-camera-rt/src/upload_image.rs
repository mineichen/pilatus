use std::io;

use minfac::ServiceCollection;
use pilatus::{
    AddFileMessage, Name, RelativeResourcePathBuf,
    device::{ActorError, ActorSystem, DynamicIdentifier},
};
use pilatus_axum::{
    DeviceJsonError, ServiceCollectionExtensions,
    extract::{InjectRegistered, Path, Query},
};

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.register_web(crate::device::DEVICE_TYPE, |r| {
        r.http("/collection/{collection_name}/{*path}", |f| {
            f.post(upload_image_to_collection)
        })
    })
}

/// Backwards-compatible upload endpoint. Behaves like the standard
/// `PUT /api/recipe/file/{device_id}/{path}`, but appends the `.png` extension for
/// clients that only send the image name. See `file_handlers` for validation.
async fn upload_image_to_collection(
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
    Path((collection_name, image_name)): Path<(Name, Name)>,
    Query(id): Query<DynamicIdentifier>,
    data: bytes::Bytes,
) -> Result<(), DeviceJsonError<io::Error>> {
    let path =
        std::path::Path::new(collection_name.as_str()).join(format!("{}.png", image_name.as_str()));
    let path = RelativeResourcePathBuf::new(path).map_err(ActorError::custom)?;

    Ok(actor_system.ask(id, AddFileMessage { path, data }).await?)
}
