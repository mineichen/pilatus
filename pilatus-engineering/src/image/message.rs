use std::sync::Arc;

pub use crate::image::{LumaImage, StableHash};
use pilatus::device::{ActorMessage, DeviceId};

use super::DynamicPointProjector;

#[derive(Default)]
#[non_exhaustive]
pub struct GetImageMessage {}

#[non_exhaustive]
pub struct GetImageOk {
    pub image: LumaImage,
    pub hash: Option<StableHash>,
}

impl GetImageOk {
    pub fn with_hash(image: LumaImage, hash: Option<StableHash>) -> Self {
        Self { image, hash }
    }
}

impl From<GetImageOk> for LumaImage {
    fn from(x: GetImageOk) -> Self {
        x.image
    }
}

impl ActorMessage for GetImageMessage {
    type Output = GetImageOk;
    type Error = anyhow::Error;
}

pub type SubscribeImageOk = Box<dyn futures::stream::Stream<Item = BroadcastImage> + Send + Sync>;

#[derive(Default)]
#[non_exhaustive]
pub struct SubscribeImageMessage {}

impl ActorMessage for SubscribeImageMessage {
    type Output = SubscribeImageOk;
    type Error = anyhow::Error;
}

/// Contains hash to be able to immediately detect changes in the producer chain
/// The consumer is free to continue the stream or reconnect
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct BroadcastImage {
    pub image: Arc<LumaImage>,
    pub hash: Option<StableHash>,
}

impl BroadcastImage {
    pub fn with_hash(image: impl Into<Arc<LumaImage>>, hash: Option<StableHash>) -> Self {
        Self {
            image: image.into(),
            hash,
        }
    }
}

impl From<GetImageOk> for BroadcastImage {
    fn from(o: GetImageOk) -> Self {
        Self {
            image: Arc::new(o.image),
            hash: o.hash,
        }
    }
}

#[derive(Default)]
#[non_exhaustive]
pub struct GetLocalizableImageMessage {}

impl ActorMessage for GetLocalizableImageMessage {
    type Output = GetLocalizableImageOk;
    type Error = anyhow::Error;
}

impl From<GetLocalizableImageMessage> for GetImageMessage {
    fn from(_: GetLocalizableImageMessage) -> Self {
        GetImageMessage {}
    }
}

#[non_exhaustive]
pub struct GetLocalizableImageOk {
    pub image: LumaImage,
    pub hash: Option<StableHash>,
    pub projector: Option<DynamicPointProjector>,
}

impl From<GetLocalizableImageOk> for BroadcastImage {
    fn from(value: GetLocalizableImageOk) -> Self {
        Self {
            image: Arc::new(value.image),
            hash: value.hash,
        }
    }
}

impl From<(Option<DynamicPointProjector>, GetImageOk)> for GetLocalizableImageOk {
    fn from((projector, image): (Option<DynamicPointProjector>, GetImageOk)) -> Self {
        Self {
            image: image.image,
            hash: image.hash,
            projector,
        }
    }
}

#[derive(Default)]
#[non_exhaustive]
pub struct SubscribeLocalizableImageMessage {}

impl ActorMessage for SubscribeLocalizableImageMessage {
    type Output = SubscribeLocalizableImageOk;
    type Error = anyhow::Error;
}

#[non_exhaustive]
pub struct SubscribeLocalizableImageOk {
    pub images: SubscribeImageOk,
    pub projector: Option<DynamicPointProjector>,
    pub image_device_id: DeviceId,
}

pub struct Device {}

impl From<SubscribeLocalizableImageOk> for SubscribeImageOk {
    fn from(value: SubscribeLocalizableImageOk) -> Self {
        value.images
    }
}

impl From<(Option<DynamicPointProjector>, SubscribeImageOk, DeviceId)>
    for SubscribeLocalizableImageOk
{
    fn from(
        (projector, images, image_device_id): (
            Option<DynamicPointProjector>,
            SubscribeImageOk,
            DeviceId,
        ),
    ) -> Self {
        Self {
            projector,
            images,
            image_device_id,
        }
    }
}

impl From<SubscribeLocalizableImageMessage> for SubscribeImageMessage {
    fn from(_: SubscribeLocalizableImageMessage) -> Self {
        Self {}
    }
}
