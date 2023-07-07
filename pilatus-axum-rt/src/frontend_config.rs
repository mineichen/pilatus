use pilatus_axum::{
    extract::{InjectAll, Json},
    DeviceTopicWebComponentLocation, IntoResponse, ServiceCollectionExtensions,
    WebComponentLocation, WebComponentLocations,
};
use serde::Serialize;

pub(super) fn register_services(c: &mut minfac::ServiceCollection) {
    c.register_web("web", |r| r.http("/config", |r| r.get(frontend_config)));
}

#[derive(Serialize)]
struct FrontendConfig {
    web_component_locations: WebComponentLocations,
}

async fn frontend_config(
    InjectAll(device_topic): InjectAll<DeviceTopicWebComponentLocation>,
    InjectAll(raw): InjectAll<WebComponentLocation>,
) -> impl IntoResponse {
    Json(FrontendConfig {
        web_component_locations: device_topic.map(Into::into).chain(raw).collect(),
    })
}
