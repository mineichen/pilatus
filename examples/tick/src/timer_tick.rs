use std::{
    num::NonZeroU64,
    sync::{Arc, Weak},
};

use minfac::{Registered, ServiceCollection};
use pilatus::{
    UpdateParamsMessage, UpdateParamsMessageError,
    device::{
        ActorMessage, ActorResult, ActorSystem, DeviceContext, DeviceResult,
        DeviceValidationContext, HandlerResult, ServiceBuilderExtensions, Step2,
        WeakUntypedActorMessageSender,
    },
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::GetTickMessage;

pub const DEVICE_TYPE: &str = "timer_tick";

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<Registered<ActorSystem>>()
        .register_device(DEVICE_TYPE, validator, device);
}

async fn validator(ctx: DeviceValidationContext<'_>) -> Result<Params, UpdateParamsMessageError> {
    ctx.params_as::<Params>()
}

async fn device(ctx: DeviceContext, params: Params, actor_system: ActorSystem) -> DeviceResult {
    let system = actor_system
        .register(ctx.id)
        .add_handler(State::update_params)
        .add_handler(State::update_tick)
        .add_handler(State::get_tick);

    if let Ok(mut self_channel) = actor_system.get_weak_untyped_sender(ctx.id) {
        let params = Arc::new(params);
        self_channel.tell(UpdateTickMessage(Arc::downgrade(&params)))?;
        system
            .execute(State {
                params,
                count: 0,
                self_channel,
            })
            .await;
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Params {
    milli_seconds_per_step: NonZeroU64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            milli_seconds_per_step: const { NonZeroU64::new(100).unwrap() },
        }
    }
}

struct State {
    self_channel: WeakUntypedActorMessageSender,
    params: Arc<Params>,
    count: u32,
}

impl State {
    async fn update_params(
        &mut self,
        msg: UpdateParamsMessage<Params>,
    ) -> ActorResult<UpdateParamsMessage<Params>> {
        *Arc::make_mut(&mut self.params) = msg.params;
        self.update_tick(UpdateTickMessage(Arc::downgrade(&self.params)))
            .await;
        Ok(())
    }
    async fn get_tick(&mut self, _msg: GetTickMessage) -> ActorResult<GetTickMessage> {
        Ok(self.count)
    }
    async fn update_tick(
        &mut self,
        UpdateTickMessage(msg_params): UpdateTickMessage,
    ) -> impl HandlerResult<UpdateTickMessage> {
        self.count += 1;
        let mut sender = self.self_channel.clone();
        Step2(async move {
            let duration = match msg_params.upgrade() {
                Some(params) => {
                    std::time::Duration::from_millis(params.milli_seconds_per_step.get())
                }
                None => return Ok(()),
            };
            tokio::time::sleep(duration).await;

            if msg_params.strong_count() > 0 {
                info!("Enqueue UpdateTickMessage after {duration:?}");
                sender.tell(UpdateTickMessage(msg_params))?;
            }

            Ok(())
        })
    }
}

struct UpdateTickMessage(Weak<Params>);
impl ActorMessage for UpdateTickMessage {
    type Output = ();
    type Error = std::convert::Infallible;
}

pub fn create_default_device_config() -> pilatus::DeviceConfig {
    pilatus::DeviceConfig::new_unchecked(DEVICE_TYPE, DEVICE_TYPE, Params::default())
}
