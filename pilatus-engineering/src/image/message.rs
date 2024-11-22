use std::{collections::HashMap, convert::Infallible, fmt::Debug, sync::Arc};

use futures::stream::BoxStream;
use pilatus::{
    device::{ActorError, ActorMessage, DeviceId},
    MissedItemsError, SubscribeMessage,
};
use serde::{Deserialize, Serialize};

use super::{
    DynamicImage, DynamicPointProjector, ImageKey, LumaImage, SpecificImageKey, StableHash,
};

#[derive(Default)]
#[non_exhaustive]
pub struct GetImageMessage {}

#[derive(Clone, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StreamImageError<TImage> {
    #[error("{0:?}")]
    MissedItems(#[from] MissedItemsError),
    #[error("Processing Error: {error:?}")]
    ProcessingError {
        image: TImage,
        error: Arc<anyhow::Error>,
    },
    #[error("ActorError: {0:?}")]
    ActorError(Arc<ActorError<Infallible>>),
}

impl<T: Debug> From<ActorError<(T, anyhow::Error)>> for StreamImageError<T> {
    fn from(value: ActorError<(T, anyhow::Error)>) -> Self {
        match value {
            ActorError::Custom((image, error)) => Self::ProcessingError {
                error: Arc::new(error),
                image,
            },
            e => Self::ActorError(Arc::new(e.map_custom(|_| unreachable!()))),
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ImageWithMeta<T> {
    pub image: T,
    pub meta: ImageMeta,
    pub other: HashMap<SpecificImageKey, T>,
}

impl<T> std::ops::Deref for ImageWithMeta<T> {
    type Target = ImageMeta;

    fn deref(&self) -> &Self::Target {
        &self.meta
    }
}

impl<T> std::ops::DerefMut for ImageWithMeta<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.meta
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ImageMeta {
    pub hash: Option<StableHash>,
}

pub type GetImageOk = ImageWithMeta<LumaImage>;

impl<T> ImageWithMeta<T> {
    pub fn with_meta(image: T, meta: ImageMeta) -> Self {
        Self {
            image,
            meta,
            other: Default::default(),
        }
    }

    pub fn with_hash(image: T, hash: Option<StableHash>) -> Self {
        Self {
            image,
            meta: ImageMeta { hash },
            other: Default::default(),
        }
    }

    pub fn with_meta_and_others(
        image: T,
        meta: ImageMeta,
        other: HashMap<SpecificImageKey, T>,
    ) -> Self {
        Self { image, meta, other }
    }

    /// ```
    /// use pilatus_engineering::image::{ImageWithMeta, ImageKey};
    ///
    /// let mut image = ImageWithMeta::with_hash((2,2), None);
    /// let bar_key: ImageKey = "bar".try_into().unwrap();
    /// image.insert(bar_key.clone(), (4,4));
    /// assert_eq!(Some(&(2,2)), image.by_name(&ImageKey::unspecified()));
    /// assert_eq!(Some(&(4,4)), image.by_name(&bar_key));
    ///
    /// image.image = (5, 5);
    /// assert_eq!(Some(&(5,5)), image.by_name(&ImageKey::unspecified()));
    /// assert_eq!(Some((5,5)), image.insert(ImageKey::unspecified(), (6,6)));
    /// assert_eq!(Some(&(6,6)), image.by_name(&ImageKey::unspecified()));
    /// ```
    pub fn by_name(&self, name: &ImageKey) -> Option<&T> {
        name.by_name_or(&self.other, &self.image)
    }

    // Returns The old value
    pub fn insert(&mut self, key: ImageKey, value: T) -> Option<T> {
        key.insert_or(value, &mut self.other, &mut self.image)
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

#[derive(Default, Debug, Clone)]
#[non_exhaustive]
pub struct SubscribeImageQuery {}

#[derive(Default)]
#[non_exhaustive]
pub struct SubscribeImageMessage {}

pub type SubscribeDynamicImageMessage = SubscribeMessage<
    SubscribeImageQuery,
    Result<ImageWithMeta<DynamicImage>, StreamImageError<DynamicImage>>,
    (),
>;

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
            hash: o.meta.hash,
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
            hash: image.meta.hash,
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
