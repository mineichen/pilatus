use minfac::{Registered, ServiceCollection};
use pilatus::{
    UpdateParamsMessageError,
    device::{
        ActorMessage, ActorResult, ActorSystem, DeviceContext, DeviceResult,
        DeviceValidationContext, DynamicIdentifier, ServiceBuilderExtensions,
    },
};
use pilatus_axum::{
    DeviceResponse, IntoResponse, ServiceCollectionExtensions,
    extract::{InjectRegistered, Query},
};

use crate::GetTickMessage;

pub const DEVICE_TYPE: &str = "manual_tick";

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<Registered<ActorSystem>>()
        .register_device(DEVICE_TYPE, validator, device);
    c.register_web("manual", |r| r.http("/increment", |f| f.put(increment_web)));
}

async fn validator(_ctx: DeviceValidationContext<'_>) -> Result<(), UpdateParamsMessageError> {
    Ok(())
}

async fn device(ctx: DeviceContext, (): (), actor_system: ActorSystem) -> DeviceResult {
    actor_system
        .register(ctx.id)
        .add_handler(State::increment_tick)
        .add_handler(State::get_tick)
        .execute(State { count: 0 })
        .await;

    Ok(())
}

struct State {
    count: u32,
}

impl State {
    async fn get_tick(&mut self, _msg: GetTickMessage) -> ActorResult<GetTickMessage> {
        Ok(self.count)
    }
    async fn increment_tick(
        &mut self,
        _msg: IncrementTickMessage,
    ) -> ActorResult<IncrementTickMessage> {
        self.count += 1;
        Ok(())
    }
}

struct IncrementTickMessage;
impl ActorMessage for IncrementTickMessage {
    type Output = ();
    type Error = std::convert::Infallible;
}

pub fn create_default_device_config() -> pilatus::DeviceConfig {
    pilatus::DeviceConfig::new_unchecked(DEVICE_TYPE, DEVICE_TYPE, serde_json::Map::default())
}

async fn increment_web(
    InjectRegistered(s): InjectRegistered<ActorSystem>,
    Query(id): Query<DynamicIdentifier>,
) -> impl IntoResponse {
    DeviceResponse::from(s.ask(id, IncrementTickMessage).await)
}
