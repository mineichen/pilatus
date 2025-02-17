use std::sync::Arc;

use futures::channel::mpsc;
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
mod permanent_recording;
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
    file_service: Arc<FileService<()>>,
    publisher: Arc<PublisherState>,
    actor_system: ActorSystem,
    recording_sender: mpsc::Sender<Option<permanent_recording::PermanentRecordingConfig>>,
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
    let system = actor_system
        .register(id)
        .add_handler(WithAbort::new(DeviceState::record))
        .add_handler(DeviceState::subscribe)
        .add_handler(DeviceState::publish_frame)
        .add_handler(DeviceState::update_params)
        .add_handler(DeviceState::list_collections)
        .add_handler(DeviceState::toggle_pause);
    let (recording_sender, permanent_recording_task) =
        permanent_recording::setup_permanent_recording(
            actor_system.get_weak_untyped_sender(id)?,
            &params.permanent_recording,
        );

    futures::future::join(
        async {
            system
                .execute(DeviceState {
                    publisher: Arc::new(PublisherState {
                        self_sender: actor_system
                            .get_weak_untyped_sender(ctx.id)
                            .expect("Just created"),

                        params,
                    }),
                    file_service: Arc::new(file_service_builder.build(ctx.id)),
                    stream: tokio::sync::broadcast::channel(1).0,
                    counter: 0,
                    paused: false,
                    actor_system,
                    recording_sender,
                })
                .await;
        },
        permanent_recording_task,
    )
    .await;

    Ok(())
}

impl DeviceState {
    async fn update_params(
        &mut self,
        UpdateParamsMessage { params }: UpdateParamsMessage<Params>,
    ) -> impl HandlerResult<UpdateParamsMessage<Params>> {
        let mutable = Arc::make_mut(&mut self.publisher);
        if mutable.params.permanent_recording != params.permanent_recording {
            if let Err(e) = self
                .recording_sender
                .try_send(params.permanent_recording.clone())
            {
                tracing::error!("Couldn't send recording task: {e}")
            };
        }

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
    permanent_recording: Option<permanent_recording::PermanentRecordingConfig>,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            active: None,
            interval: 500,
            file_ending: "png".into(),
            permanent_recording: None,
        }
    }
}

pub fn create_default_device_config() -> pilatus::DeviceConfig {
    pilatus::DeviceConfig::new_unchecked(DEVICE_TYPE, DEVICE_TYPE, Params::default())
}
