use std::sync::Arc;

use minfac::{Registered, ServiceCollection};
use pilatus::device::{HandlerResult, Step2, WithAbort};
use pilatus::{
    device::{ActorSystem, DeviceContext, DeviceResult, DeviceValidationContext},
    prelude::*,
    UpdateParamsMessage, UpdateParamsMessageError,
};
use pilatus::{FileService, FileServiceBuilder, Name};
use pilatus_engineering::image::{DynamicImage, ImageWithMeta, StreamImageError};
use publish_frame::PublisherState;
use serde::{Deserialize, Serialize};

mod list_collections;
mod pause;
mod publish_frame;
mod record;
mod subscribe;

pub const DEVICE_TYPE: &str = "engineering-emulation-camera";

pub(super) fn register_services(c: &mut ServiceCollection) {
    record::register_services(c);
    pause::register_services(c);
    c.with::<(Registered<ActorSystem>, Registered<FileServiceBuilder>)>()
        .register_device(DEVICE_TYPE, validator, device);
}

struct DeviceState {
    counter: u32,
    paused: bool,
    stream: tokio::sync::broadcast::Sender<
        Result<ImageWithMeta<DynamicImage>, StreamImageError<DynamicImage>>,
    >,
    file_service: FileService<()>,
    publisher: Arc<PublisherState>,
    actor_system: ActorSystem,
}

async fn validator(ctx: DeviceValidationContext<'_>) -> Result<Params, UpdateParamsMessageError> {
    ctx.params_as::<Params>()
}

async fn device(
    ctx: DeviceContext,
    params: Params,
    (actor_system, file_service_builder): (ActorSystem, FileServiceBuilder),
) -> DeviceResult {
    let id = ctx.id;

    actor_system
        .register(id)
        .add_handler(WithAbort::new(DeviceState::record))
        .add_handler(DeviceState::subscribe)
        .add_handler(DeviceState::publish_frame)
        .add_handler(DeviceState::update_params)
        .add_handler(DeviceState::list_collections)
        .add_handler(DeviceState::toggle_pause)
        .execute(DeviceState {
            publisher: Arc::new(PublisherState {
                self_sender: actor_system
                    .get_weak_untyped_sender(ctx.id)
                    .expect("Just created"),

                params,
            }),
            file_service: file_service_builder.build(ctx.id),
            stream: tokio::sync::broadcast::channel(1).0,
            counter: 0,
            paused: false,
            actor_system: actor_system.clone(),
        })
        .await;

    Ok(())
}

impl DeviceState {
    async fn update_params(
        &mut self,
        UpdateParamsMessage { params }: UpdateParamsMessage<Params>,
    ) -> impl HandlerResult<UpdateParamsMessage<Params>> {
        let mutable = Arc::make_mut(&mut self.publisher);
        mutable.params = params;
        let weak = Arc::downgrade(&self.publisher);

        Step2(async {
            PublisherState::send_delayed(weak).await;
            Ok(())
        })
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct Params {
    active: Option<Name>,
    interval: u64,
    file_ending: String,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            active: None,
            interval: 500,
            file_ending: "png".into(),
        }
    }
}

pub fn create_default_device_config() -> pilatus::DeviceConfig {
    pilatus::DeviceConfig::new_unchecked(DEVICE_TYPE, DEVICE_TYPE, Params::default())
}
