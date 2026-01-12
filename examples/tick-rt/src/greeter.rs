use minfac::{Registered, ServiceCollection};
use pilatus::{
    UpdateParamsMessage, UpdateParamsMessageError,
    device::{
        ActorMessage, ActorResult, ActorSystem, DeviceContext, DeviceResult,
        DeviceValidationContext, DynamicIdentifier, HandlerResult, ServiceBuilderExtensions,
    },
};
use pilatus_axum::{
    DeviceResponse, IntoResponse, ServiceCollectionExtensions,
    extract::{InjectRegistered, Path, Query},
};
use pilatus_tick::{GreeterLanguage, GreeterParams};

use crate::GetTickMessage;

pub const DEVICE_TYPE: &str = "pilatus-greeter";

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<Registered<ActorSystem>>()
        .register_device(DEVICE_TYPE, validator, device);
    c.register_web(DEVICE_TYPE, |r| r.http("/greet/{name}", |f| f.get(greet_web)));
}

async fn validator(
    ctx: DeviceValidationContext<'_>,
) -> Result<GreeterParams, UpdateParamsMessageError> {
    ctx.params_as::<GreeterParams>()
}

async fn device(
    ctx: DeviceContext,
    params: GreeterParams,
    actor_system: ActorSystem,
) -> DeviceResult {
    actor_system
        .register(ctx.id)
        .add_handler(State::update_params)
        .add_handler(State::greet)
        .execute(State {
            params,
            actor_system,
        })
        .await;

    Ok(())
}

struct State {
    actor_system: ActorSystem,
    params: GreeterParams,
}

impl State {
    async fn update_params(
        &mut self,
        msg: UpdateParamsMessage<GreeterParams>,
    ) -> ActorResult<UpdateParamsMessage<GreeterParams>> {
        self.params = msg.params;
        Ok(())
    }
    async fn greet(&mut self, msg: GreetMessage) -> impl HandlerResult<GreetMessage> {
        let tick = self
            .actor_system
            .ask(DynamicIdentifier::None, GetTickMessage)
            .await?;

        Ok(format!(
            "{} {} (generation: {tick})\n",
            match self.params.lang {
                GreeterLanguage::English => "Hello",
                GreeterLanguage::German => "Hallo",
            },
            msg.name,
        ))
    }
}

struct GreetMessage {
    name: String,
}
impl ActorMessage for GreetMessage {
    type Output = String;
    type Error = std::convert::Infallible;
}

pub fn create_default_device_config() -> pilatus::DeviceConfig {
    pilatus::DeviceConfig::new_unchecked(DEVICE_TYPE, DEVICE_TYPE, GreeterParams::default())
}

async fn greet_web(
    InjectRegistered(s): InjectRegistered<ActorSystem>,
    Query(id): Query<DynamicIdentifier>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    DeviceResponse::from(s.ask(id, GreetMessage { name }).await)
}
