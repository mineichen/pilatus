use std::sync::Arc;

pub use crate::image::{LumaImage, StableHash};
use pilatus::device::ActorMessage;

pub struct GetImageMessage;

pub struct GetImageMessageOutput {
    pub image: LumaImage,
    pub quality_hash: Option<StableHash>,
}

impl From<GetImageMessageOutput> for LumaImage {
    fn from(x: GetImageMessageOutput) -> Self {
        x.image
    }
}

impl ActorMessage for GetImageMessage {
    type Output = GetImageMessageOutput;
    type Error = anyhow::Error;
}

#[derive(Default)]
pub struct SubscribeImageMessage {}

impl ActorMessage for SubscribeImageMessage {
    type Output = Box<dyn futures::stream::Stream<Item = BroadcastImage> + Send + Sync>;
    type Error = ();
}

#[derive(Debug, Clone)]
pub struct BroadcastImage {
    pub image: Arc<LumaImage>,
    pub quality_hash: Option<StableHash>,
}

impl From<GetImageMessageOutput> for BroadcastImage {
    fn from(o: GetImageMessageOutput) -> Self {
        Self {
            image: Arc::new(o.image),
            quality_hash: o.quality_hash,
        }
    }
}
