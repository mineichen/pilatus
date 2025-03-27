use std::{collections::HashMap, convert::Infallible, fmt::Debug, sync::Arc};

use futures::stream::BoxStream;
use pilatus::{
    device::{ActorError, ActorMessage, DeviceId},
    MissedItemsError, SubscribeMessage,
};

use super::{
    DynamicImage, DynamicPointProjector, ImageKey, LumaImage, SpecificImageKey, StableHash,
};

mod meta;

pub use meta::*;

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

    /// Ok if the key is found or unspecified, Err if a key was specified but not found (returning the main image then)
    /// ```
    /// use pilatus_engineering::image::{ImageWithMeta, ImageKey};
    ///
    /// let mut image = ImageWithMeta::with_hash((2,2), None);
    /// let bar_key: ImageKey = "bar".try_into().unwrap();
    /// image.insert(bar_key.clone(), (4,4));
    /// assert_eq!(&(2,2), image.by_key(&ImageKey::unspecified()).unwrap());
    /// assert_eq!(&(4,4), image.by_key(&bar_key).unwrap());
    ///
    /// image.image = (5, 5);
    /// assert_eq!(&(5,5), image.by_key(&ImageKey::unspecified()).unwrap());
    /// assert_eq!(Some((5,5)), image.insert(ImageKey::unspecified(), (6,6)));
    /// assert_eq!(&(6,6), image.by_key(&ImageKey::unspecified()).unwrap());
    /// ```
    pub fn by_key<'a>(&'a self, search_key: &'a ImageKey) -> Result<&'a T, UnknownKeyError<'a, T>>
    where
        T: Debug,
    {
        search_key
            .by_key_or(&self.other, &self.image)
            .ok_or_else(|| UnknownKeyError {
                main_image: &self.image,
                search_key,
                available_keys: self.other.keys(),
            })
    }

    // Returns The old value
    pub fn insert(&mut self, key: ImageKey, value: T) -> Option<T> {
        key.insert_or(value, &mut self.other, &mut self.image)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Unknown image key: {search_key:?}. Available are {available_keys:?}")]
pub struct UnknownKeyError<'a, T: Debug> {
    pub main_image: &'a T,
    pub search_key: &'a ImageKey,
    pub available_keys: std::collections::hash_map::Keys<'a, SpecificImageKey, T>,
}

impl<T: Debug + Clone + Send + Sync> From<UnknownKeyError<'_, T>> for (T, anyhow::Error) {
    fn from(val: UnknownKeyError<'_, T>) -> Self {
        let image = val.main_image.clone();
        let error = anyhow::anyhow!("{val:?}");
        (image, error)
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
