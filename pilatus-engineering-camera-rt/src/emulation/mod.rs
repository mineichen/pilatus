use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use futures::channel::mpsc;
use futures::FutureExt;
use minfac::{Registered, ServiceCollection};
use pilatus::device::{DeviceId, HandlerResult, Step2, WithAbort};
use pilatus::{
    device::{ActorSystem, DeviceContext, DeviceResult, DeviceValidationContext},
    prelude::*,
    UpdateParamsMessage, UpdateParamsMessageError,
};
use pilatus::{FileService, FileServiceBuilder, Name};
use pilatus_engineering::image::{DynamicImage, ImageWithMeta, StreamImageError};
use publish_frame::PublisherState;
use serde::{Deserialize, Serialize};
use tracing::warn;

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
                if self.publisher.params.permanent_recording != params.permanent_recording {
                    if let Err(e) = self
                        .recording_sender
                        .try_send(params.permanent_recording.clone())
                    {
                        tracing::error!("Couldn't send recording task: {e}")
                    };
                }
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

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Params {
    mode: EmulationMode,
    file: FileParams,
    streaming: StreamingParams,
    permanent_recording: Option<permanent_recording::PermanentRecordingConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields)]
enum EmulationMode {
    #[default]
    File,
    Streaming,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields, default)]
struct FileParams {
    active: ActiveRecipe,
    auto_restart: bool,
    file_ending: String,
    interval: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields, default)]
struct StreamingParams {
    source_device_id: Option<DeviceId>,
}

impl Default for FileParams {
    fn default() -> Self {
        Self {
            active: Default::default(),
            auto_restart: true,
            file_ending: "png".into(),
            interval: 500,
        }
    }
}

/// Strings which are valid Names, so don't contain any slashes/backward-slashes, are interpreted as recorded collections. Otherwise it's assumed to be a path. Use ./foo if you want a folder located in $PWD
#[derive(Default, Debug, Clone, PartialEq)]
enum ActiveRecipe {
    #[default]
    Undefined,
    Named(Name),
    External(PathBuf),
}

impl Serialize for ActiveRecipe {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ActiveRecipe::Undefined => Option::<()>::None.serialize(serializer),
            ActiveRecipe::Named(name_wrapper) => name_wrapper.as_str().serialize(serializer),
            ActiveRecipe::External(path_buf) => path_buf.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ActiveRecipe {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match Option::<String>::deserialize(deserializer)? {
            Some(x) => match Name::from_str(&x) {
                Ok(x) => Ok(Self::Named(x)),
                Err(_) => Ok(Self::External(PathBuf::from(x))),
            },
            None => Ok(Self::Undefined),
        }
    }
}

pub fn create_default_device_config() -> pilatus::DeviceConfig {
    pilatus::DeviceConfig::new_unchecked(DEVICE_TYPE, DEVICE_TYPE, Params::default())
}
