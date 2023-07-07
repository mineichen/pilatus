use std::borrow::Cow;

use serde::{ser::SerializeMap, Serialize};

#[derive(serde::Serialize)]
pub struct DeviceTopicWebComponentLocation {
    pub device_type: &'static str,
    pub topic: &'static str,
    pub target: &'static str,
}

/// Specify locations for webcomponents of any kind
/// Just the first part before the slash is considered for mapping
/// - for 'my-device/test', only 'my-device' should be mapped
/// - for 'other', the entire word 'other' should be mapped
pub struct WebComponentLocation(WebComponentLocationKind);

impl WebComponentLocation {
    pub fn new(
        component_id: impl Into<Cow<'static, str>>,
        target: impl Into<Cow<'static, str>>,
    ) -> Self {
        WebComponentLocation(WebComponentLocationKind::Raw(
            component_id.into(),
            target.into(),
        ))
    }
}

enum WebComponentLocationKind {
    DeviceTopic(DeviceTopicWebComponentLocation),
    Raw(Cow<'static, str>, Cow<'static, str>),
}

pub struct WebComponentLocations(Vec<WebComponentLocation>);
impl Serialize for WebComponentLocations {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        for item in self.0.iter() {
            match &item.0 {
                WebComponentLocationKind::DeviceTopic(d) => {
                    map.serialize_entry(&format_args!("{}/{}", d.device_type, d.topic), d.target)?
                }
                WebComponentLocationKind::Raw(component_id, target) => {
                    map.serialize_entry(component_id, target)?
                }
            };
        }
        map.end()
    }
}

impl FromIterator<WebComponentLocation> for WebComponentLocations {
    fn from_iter<T: IntoIterator<Item = WebComponentLocation>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl From<DeviceTopicWebComponentLocation> for WebComponentLocation {
    fn from(value: DeviceTopicWebComponentLocation) -> Self {
        WebComponentLocation(WebComponentLocationKind::DeviceTopic(value))
    }
}
