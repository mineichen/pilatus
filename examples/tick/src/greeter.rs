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
use serde::{Deserialize, Serialize};

use crate::GetTickMessage;

pub const DEVICE_TYPE: &str = "greeter";

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<Registered<ActorSystem>>()
        .register_device(DEVICE_TYPE, validator, device);
    c.register_web("greeter", |r| r.http("/greet/{name}", |f| f.get(greet_web)));
}

async fn validator(ctx: DeviceValidationContext<'_>) -> Result<Params, UpdateParamsMessageError> {
    ctx.params_as::<Params>()
}

async fn device(ctx: DeviceContext, params: Params, actor_system: ActorSystem) -> DeviceResult {
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

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Params {
    lang: Language,
}

#[derive(Debug, Default, Deserialize, Serialize)]
enum Language {
    #[default]
    English,
    German,
}

struct State {
    actor_system: ActorSystem,
    params: Params,
}

impl State {
    async fn update_params(
        &mut self,
        msg: UpdateParamsMessage<Params>,
    ) -> ActorResult<UpdateParamsMessage<Params>> {
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
                Language::English => "Hello",
                Language::German => "Hallo",
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
    pilatus::DeviceConfig::new_unchecked(DEVICE_TYPE, DEVICE_TYPE, Params::default())
}

async fn greet_web(
    InjectRegistered(s): InjectRegistered<ActorSystem>,
    Query(id): Query<DynamicIdentifier>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    DeviceResponse::from(s.ask(id, GreetMessage { name }).await)
}
