use std::sync::Arc;

use futures::stream::BoxStream;
use pilatus::device::{ActorMessage, DeviceId};

use super::{DynamicPointProjector, LumaImage, StableHash};

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

pub type SubscribeImageOk = BoxStream<'static, BroadcastImage>;

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

impl From<LocalizableBroadcastImage> for BroadcastImage {
    fn from(value: LocalizableBroadcastImage) -> Self {
        Self {
            image: value.image,
            hash: value.hash,
        }
    }
}

/// Contains hash to be able to immediately detect changes in the producer chain
/// The consumer is free to continue the stream or reconnect
#[non_exhaustive]
#[derive(Clone)]
pub struct LocalizableBroadcastImage {
    pub image: Arc<LumaImage>,
    pub hash: Option<StableHash>,
    pub projector: Option<DynamicPointProjector>,
}

impl LocalizableBroadcastImage {
    pub fn with_hash_and_projector(
        image: impl Into<Arc<LumaImage>>,
        hash: Option<StableHash>,
        projector: Option<DynamicPointProjector>,
    ) -> Self {
        Self {
            image: image.into(),
            hash,
            projector,
        }
    }
}

impl std::fmt::Debug for LocalizableBroadcastImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalizableBroadcastImage")
            .field("image", &self.image)
            .field("hash", &self.hash)
            .finish()
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
    pub images: BoxStream<'static, LocalizableBroadcastImage>,
    pub image_device_id: DeviceId,
}

impl From<SubscribeLocalizableImageOk> for BoxStream<'static, LocalizableBroadcastImage> {
    fn from(value: SubscribeLocalizableImageOk) -> Self {
        value.images
    }
}

impl From<(BoxStream<'static, LocalizableBroadcastImage>, DeviceId)>
    for SubscribeLocalizableImageOk
{
    fn from(
        (images, image_device_id): (BoxStream<'static, LocalizableBroadcastImage>, DeviceId),
    ) -> Self {
        Self {
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
