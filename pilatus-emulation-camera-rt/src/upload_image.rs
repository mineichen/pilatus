use minfac::ServiceCollection;
use pilatus::{
    Name, RelativeFilePath,
    device::{ActorError, ActorMessage, ActorResult, ActorSystem, DynamicIdentifier},
};
use pilatus_axum::{
    DeviceJsonError, ServiceCollectionExtensions,
    extract::{InjectRegistered, Path, Query},
};
use pilatus_engineering::image::ImageEncoderTrait;

use crate::DeviceState;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.register_web(crate::device::DEVICE_TYPE, |r| {
        r.http("/collection/{collection_name}/{*path}", |f| {
            f.post(upload_image_to_collection)
        })
    })
}

pub(super) struct AddImageMessage {
    pub collection_name: Name,
    pub image_name: Name,
    pub image: pilatus_engineering::image::DynamicImage,
}

impl ActorMessage for AddImageMessage {
    type Output = ();
    type Error = anyhow::Error;
}

impl DeviceState {
    pub(super) async fn add_image(&mut self, msg: AddImageMessage) -> ActorResult<AddImageMessage> {
        let relative = RelativeFilePath::new(
            std::path::Path::new(msg.collection_name.as_str())
                .join(format!("{}.png", msg.image_name.as_str())),
        )
        .expect("Name is always a valid path");
        let encoder = self.encoder.clone();
        let encoded_image = pilatus::execute_blocking(move || encoder.encode(msg.image))
            .await
            .map_err(ActorError::custom)?;
        self.file_service
            .add_file_unchecked(&relative, &encoded_image)
            .await?;
        Ok(())
    }
}

async fn upload_image_to_collection(
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
    Path((collection_name, image_name)): Path<(Name, Name)>,
    Query(id): Query<DynamicIdentifier>,
    data: bytes::Bytes,
) -> Result<(), DeviceJsonError<anyhow::Error>> {
    let decode_image = pilatus::execute_blocking(move || image::load_from_memory(&data))
        .await
        .map_err(ActorError::custom)?;

    let image = pilatus_engineering::image::DynamicImage::try_from(decode_image)
        .map_err(ActorError::custom)?;

    Ok(actor_system
        .ask(
            id,
            AddImageMessage {
                image_name,
                image,
                collection_name,
            },
        )
        .await?)
}
