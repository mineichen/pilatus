use std::sync::Arc;

use futures::FutureExt;
use futures::channel::mpsc;
use minfac::{Registered, ServiceCollection};
use pilatus::device::{HandlerResult, Step2, WithAbort};
use pilatus::{FileService, FileServiceBuilder};
use pilatus::{
    UpdateParamsMessage, UpdateParamsMessageError,
    device::{ActorSystem, DeviceContext, DeviceResult, DeviceValidationContext},
    prelude::*,
};
use pilatus_engineering::image::{DynamicImage, ImageWithMeta, StreamImageError};
use tracing::warn;

use crate::publish_frame::PublisherState;
use crate::{EmulationMode, Params};

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<(Registered<ActorSystem>, Registered<FileServiceBuilder>)>()
        .register_device(DEVICE_TYPE, validator, device);
}

pub const DEVICE_TYPE: &str = "engineering-emulation-camera";

pub extern "C" fn register(c: &mut ServiceCollection) {
    crate::record::register_services(c);
    crate::device::register_services(c);
    crate::pause::register_services(c);
}

pub(super) struct DeviceState {
    pub(crate) paused: bool,
    pub(crate) stream: tokio::sync::broadcast::Sender<
        Result<ImageWithMeta<DynamicImage>, StreamImageError<DynamicImage>>,
    >,
    pub(crate) file_service: Arc<FileService<()>>,
    pub(crate) publisher: Arc<PublisherState>,
    pub(crate) actor_system: ActorSystem,
    pub(crate) recording_sender:
        mpsc::Sender<Option<crate::permanent_recording::PermanentRecordingConfig>>,
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
        crate::permanent_recording::setup_permanent_recording(
            actor_system.get_weak_untyped_sender(id)?,
            if params.mode == EmulationMode::File {
                &params.permanent_recording
            } else {
                &None
            },
        );

    futures::future::join(
        async {
            system
                .execute(DeviceState {
                    publisher: Arc::new(PublisherState::new(
                        actor_system
                            .get_weak_untyped_sender(ctx.id)
                            .expect("Just created"),
                        params,
                    )),
                    file_service: Arc::new(file_service_builder.build(ctx.id)),
                    stream: tokio::sync::broadcast::channel(1).0,
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
        match params.mode {
            EmulationMode::File => {
                if self.publisher.params.permanent_recording != params.permanent_recording
                    && let Err(e) = self
                        .recording_sender
                        .try_send(params.permanent_recording.clone())
                {
                    tracing::error!("Couldn't send recording task: {e}")
                };
                match Arc::get_mut(&mut self.publisher) {
                    Some(old)
                        if old.params.file.active == params.file.active
                            && old.params.file.file_ending == params.file.file_ending =>
                    {
                        old.params = params;
                    }
                    _ => self.publisher = Arc::new(self.publisher.with_params(params)),
                }
                let weak = Arc::downgrade(&self.publisher);

                Step2(
                    async {
                        PublisherState::send_delayed(weak).await;
                        Ok(())
                    }
                    .boxed(),
                )
            }
            EmulationMode::Streaming => {
                warn!("Updating Stream params not yet supported. Update is ignored");
                Step2(std::future::ready(Ok(())).boxed())
            }
        }
    }
}

pub fn create_default_device_config() -> pilatus::DeviceConfig {
    pilatus::DeviceConfig::new_unchecked(DEVICE_TYPE, DEVICE_TYPE, Params::default())
}
