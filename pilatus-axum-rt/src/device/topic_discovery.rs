use pilatus_axum::{
    extract::{InjectAll, Json},
    DeviceTopicDiscovery, IntoResponse, ServiceCollectionExtensions,
};
use serde::Serialize;

pub(super) fn register_services(c: &mut minfac::ServiceCollection) {
    c.register_web("web", |r| r.http("/config", |r| r.get(component_paths)));
}

#[derive(Serialize)]
struct DeviceTopicDiscoveryModel {
    pub device_type: &'static str,
    pub topic: &'static str,
    pub path: &'static str,
}

impl From<DeviceTopicDiscovery> for DeviceTopicDiscoveryModel {
    fn from(
        DeviceTopicDiscovery {
            device_type,
            topic,
            path,
        }: DeviceTopicDiscovery,
    ) -> Self {
        Self {
            device_type,
            topic,
            path,
        }
    }
}

#[derive(Serialize)]
struct WebConfig<TDeviceTopics: IntoIterator<Item = DeviceTopicDiscoveryModel> + Serialize> {
    // (DeviceType, Topic), JsLoaderPath
    device_topics: TDeviceTopics,
}

async fn component_paths(
    InjectAll(discoveries): InjectAll<DeviceTopicDiscovery>,
) -> impl IntoResponse {
    Json(WebConfig {
        device_topics: discoveries
            .map(DeviceTopicDiscoveryModel::from)
            .collect::<Vec<_>>(),
    })
}
