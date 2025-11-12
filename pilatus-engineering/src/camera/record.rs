use std::num::NonZeroU32;

use pilatus::device::{ActorMessage, DeviceId};

#[derive(Debug)]
#[non_exhaustive]
pub struct RecordMessage {
    pub source_id: DeviceId,
    pub collection_name: pilatus::Name,
    pub max_size_mb: Option<NonZeroU32>,
}

impl RecordMessage {
    pub fn with_option_max_size(
        source_id: DeviceId,
        collection_name: pilatus::Name,
        max_size_mb: Option<NonZeroU32>,
    ) -> anyhow::Result<Self> {
        match max_size_mb.map(NonZeroU32::get) {
            Some(100_001..) => Err(anyhow::anyhow!("max_size_mb > 100_000")),
            _ => Ok(Self {
                source_id,
                collection_name,
                max_size_mb,
            }),
        }
    }
    pub fn with_max_size(
        source_id: DeviceId,
        collection_name: pilatus::Name,
        max_size_mb: NonZeroU32,
    ) -> Self {
        Self {
            source_id,
            collection_name,
            max_size_mb: Some(max_size_mb),
        }
    }
}

impl ActorMessage for RecordMessage {
    type Output = ();
    type Error = anyhow::Error;
}
