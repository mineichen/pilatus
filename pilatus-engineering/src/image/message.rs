use std::{convert::Infallible, fmt::Debug, marker::PhantomData, sync::Arc};

use futures::stream::BoxStream;
use imbuf::PixelType;
use pilatus::{
    device::{ActorError, ActorMessage, CustomActorError, DeviceId},
    MissedItemsError,
};

use super::{DynamicImage, DynamicPointProjector, Image, ImageWithMeta, LumaImage, StableHash};

#[derive(Clone, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StreamImageError<TImage> {
    #[error("{0:?}")]
    #[deprecated = "Tracked in MetaData::missed_frames instead, as this is often not a problem in real applications"]
    MissedItems(#[from] MissedItemsError),
    #[error("Processing Error: {error:?}")]
    ProcessingError {
        image: TImage,
        error: Arc<anyhow::Error>,
    },
    #[error("Error during image acquisition")]
    Acquisition { error: Arc<anyhow::Error> },
    #[error("ActorError: {0:?}")]
    ActorError(Arc<ActorError<Infallible>>),
}

impl<T> CustomActorError for StreamImageError<T> {}

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

impl<TSrc> StreamImageError<TSrc> {
    pub fn try_convert_image<TDst>(self) -> Result<StreamImageError<TDst>, TDst::Error>
    where
        TDst: TryFrom<TSrc>,
    {
        Ok(match self {
            StreamImageError::ProcessingError { image, error } => {
                StreamImageError::ProcessingError {
                    image: image.try_into()?,
                    error: error,
                }
            }
            StreamImageError::Acquisition { error } => StreamImageError::Acquisition { error },
            StreamImageError::ActorError(error) => StreamImageError::ActorError(error),
            #[expect(deprecated)]
            StreamImageError::MissedItems(x) => StreamImageError::MissedItems(x),
        })
    }
}

#[derive(Default)]
#[non_exhaustive]
pub struct GetImageMessage {}
pub type GetImageOk = ImageWithMeta<LumaImage>;

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

/// Subscribe to images of any pixel type and number of channels.
/// Use [`SubscribeImageWithMetaMessage`] if the type is known upfront.
///
/// Prefer messages, which are generic over the [`PixelType`] and the number of channels, over messages, which are
/// generic over image types like [`DynamicImage`] (e.g. `SubscribeMessage<Q, DynamicImage, E>`): The `'static`
/// lifetime of images is erased, when the compiler checks whether an `async` block is `Send`. Awaiting a generic
/// `async fn` whose generic arguments contain an image type is therefore `!Send` ("implementation of `Send` is not
/// general enough"). `ActorSystem::ask` returns `impl Future + Send + 'static` to support them anyway.
#[derive(Default)]
#[non_exhaustive]
pub struct SubscribeDynamicImageMessage {
    pub query: SubscribeImageQuery,
}

impl From<SubscribeImageQuery> for SubscribeDynamicImageMessage {
    fn from(query: SubscribeImageQuery) -> Self {
        Self { query }
    }
}

impl ActorMessage for SubscribeDynamicImageMessage {
    type Output =
        BoxStream<'static, Result<ImageWithMeta<DynamicImage>, StreamImageError<DynamicImage>>>;
    type Error = Infallible;
}

/// Typed version of [`SubscribeDynamicImageMessage`]
#[non_exhaustive]
pub struct SubscribeImageWithMetaMessage<T, const CHANNELS: usize> {
    pub query: SubscribeImageQuery,
    phantom: PhantomData<T>,
}

impl<T, const CHANNELS: usize> Default for SubscribeImageWithMetaMessage<T, CHANNELS> {
    fn default() -> Self {
        SubscribeImageQuery::default().into()
    }
}

impl<T, const CHANNELS: usize> From<SubscribeImageQuery>
    for SubscribeImageWithMetaMessage<T, CHANNELS>
{
    fn from(query: SubscribeImageQuery) -> Self {
        Self {
            query,
            phantom: PhantomData,
        }
    }
}

impl<T: PixelType, const CHANNELS: usize> ActorMessage
    for SubscribeImageWithMetaMessage<T, CHANNELS>
{
    type Output = BoxStream<
        'static,
        Result<ImageWithMeta<Image<T, CHANNELS>>, StreamImageError<Image<T, CHANNELS>>>,
    >;
    type Error = Infallible;
}

impl ActorMessage for SubscribeImageMessage {
    type Output = SubscribeImageOk;
    type Error = anyhow::Error;
}

/// Contains hash to be able to immediately detect changes in the producer chain
/// The consumer is free to continue the stream or reconnect
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct BroadcastImage {
    pub image: LumaImage,
    pub hash: Option<StableHash>,
}

impl BroadcastImage {
    pub fn with_hash(image: LumaImage, hash: Option<StableHash>) -> Self {
        Self { image, hash }
    }
}

impl From<GetImageOk> for BroadcastImage {
    fn from(o: GetImageOk) -> Self {
        Self {
            image: o.image,
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
    pub image: LumaImage,
    pub hash: Option<StableHash>,
    pub projector: Option<DynamicPointProjector>,
}

impl LocalizableBroadcastImage {
    pub fn with_hash_and_projector(
        image: LumaImage,
        hash: Option<StableHash>,
        projector: Option<DynamicPointProjector>,
    ) -> Self {
        Self {
            image,
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
            image: value.image,
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

#[cfg(test)]
mod tests {
    use pilatus::{
        device::{ActorSystem, DynamicIdentifier},
        SubscribeMessage,
    };

    use super::*;

    fn assert_send<T: Send>(_: T) {}
    fn assert_send_static<T: Send + 'static>(_: T) {}

    /// Compile-time check: The `'static` lifetime of images is erased, when checking whether an async block is
    /// `Send`. Unless `ActorSystem::ask` declares `Send`, asking with messages, which are generic over an image
    /// type, fails with "implementation of `Send` is not general enough"
    #[test]
    fn asking_for_images_is_send() {
        let system = ActorSystem::new();
        let id = DynamicIdentifier::None;
        assert_send_static(system.ask(&id, SubscribeDynamicImageMessage::default()));

        assert_send(async move {
            let _ = system
                .ask(
                    DynamicIdentifier::None,
                    SubscribeDynamicImageMessage::default(),
                )
                .await;
            let _ = system
                .ask(
                    DynamicIdentifier::None,
                    SubscribeImageWithMetaMessage::<u8, 1>::default(),
                )
                .await;
            let _ = system
                .ask(
                    DynamicIdentifier::None,
                    SubscribeMessage::<SubscribeImageQuery, DynamicImage, Infallible>::default(),
                )
                .await;
        });
    }
}
