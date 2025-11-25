use minfac::ServiceCollection;
use pilatus::device::{ActorMessage, ActorResult, ActorSystem, DynamicIdentifier};
use pilatus_axum::{
    DeviceResponse, IntoResponse, ServiceCollectionExtensions,
    extract::{InjectRegistered, Query},
};

use super::DeviceState;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.register_web("engineering/emulation-camera", |r| {
        r.http("/toggle_pause", |f| f.put(toggle_pause_web))
    })
}

pub(super) struct TogglePauseMessage;

impl ActorMessage for TogglePauseMessage {
    type Output = ();
    type Error = std::convert::Infallible;
}

impl DeviceState {
    pub(super) async fn toggle_pause(
        &mut self,
        _msg: TogglePauseMessage,
    ) -> ActorResult<TogglePauseMessage> {
        self.paused = !self.paused;
        Ok(())
    }
}

async fn toggle_pause_web(
    InjectRegistered(actor_system): InjectRegistered<ActorSystem>,
    Query(id): Query<DynamicIdentifier>,
) -> impl IntoResponse {
    DeviceResponse::from(actor_system.ask(id, TogglePauseMessage).await)
}
