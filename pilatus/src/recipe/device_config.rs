use serde::{Deserialize, Serialize};

use crate::{Name, TransactionError, UntypedDeviceParamsWithVariables};

#[derive(Debug, PartialEq, Eq, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceConfig {
    pub device_type: String,
    pub device_name: Name,
    pub params: UntypedDeviceParamsWithVariables,

    /// Stores the original Parameters if parameters are saved uncommitted
    #[serde(skip_serializing_if = "Option::is_none")]
    committed_params: Option<UntypedDeviceParamsWithVariables>,
}

#[derive(thiserror::Error, Debug)]
#[error("No committed configuration found")]
pub struct NoCommittedConfigurationFound;

impl From<NoCommittedConfigurationFound> for TransactionError {
    fn from(e: NoCommittedConfigurationFound) -> Self {
        Self::Other(e.into())
    }
}

impl DeviceConfig {
    pub fn new<S: Serialize>(
        device_type: impl Into<String>,
        device_name: Name,
        params: S,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            device_type: device_type.into(),
            device_name,
            params: UntypedDeviceParamsWithVariables::from_serializable(&params)?,
            committed_params: None,
        })
    }

    pub fn with_name(self, name: Name) -> Self {
        Self {
            device_name: name,
            ..self
        }
    }

    pub fn new_unchecked(
        device_type: impl Into<String>,
        device_name: impl Into<String>,
        params: impl Serialize,
    ) -> Self {
        Self::new(
            device_type,
            Name::new(device_name).expect("DeviceType is valid name"),
            params,
        )
        .expect("DeviceParams can be serialized")
    }

    pub fn update_params_committed(&mut self, params: UntypedDeviceParamsWithVariables) {
        self.params = params;
        self.committed_params = None;
    }

    pub fn update_params_uncommitted(&mut self, mut params: UntypedDeviceParamsWithVariables) {
        std::mem::swap(&mut params, &mut self.params);
        if self.committed_params.is_none() {
            self.committed_params = Some(params);
        }
    }

    pub fn restore_committed(
        &mut self,
    ) -> Result<&UntypedDeviceParamsWithVariables, NoCommittedConfigurationFound> {
        if let Some(x) = self.committed_params.as_mut() {
            std::mem::swap(x, &mut self.params);
            self.committed_params = None;
            Ok(&self.params)
        } else {
            Err(NoCommittedConfigurationFound)
        }
    }

    pub fn get_device_type(&self) -> &str {
        &self.device_type
    }

    #[cfg(any(test, feature = "unstable"))]
    pub fn mock(params: impl Serialize) -> Self {
        Self {
            device_type: "testdevice".into(),
            device_name: Name::new("testdevicename").unwrap(),
            params: UntypedDeviceParamsWithVariables::from_serializable(&params).unwrap(),
            committed_params: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_committed() {
        let mut p = DeviceConfig::mock(1);
        p.update_params_uncommitted(
            UntypedDeviceParamsWithVariables::from_serializable(42).unwrap(),
        );
        assert_eq!(Some(42), p.params.0.as_i64());
        p.update_params_uncommitted(
            UntypedDeviceParamsWithVariables::from_serializable(10).unwrap(),
        );
        assert_eq!(Some(10), p.params.0.as_i64());
        p.restore_committed().unwrap();
        assert_eq!(Some(1), p.params.0.as_i64());
        assert!(p.restore_committed().is_err());
    }

    #[test]
    fn test_read_write_params() {
        #[derive(Serialize, Deserialize)]
        struct MyParams {
            foo: String,
            bar: u8,
        }

        let device = DeviceConfig::new_unchecked(
            "myDeviceType",
            "myDeviceName",
            MyParams {
                foo: "hallo".to_string(),
                bar: 9,
            },
        );

        let p = serde_json::from_value::<MyParams>(device.params.0).unwrap();
        assert_eq!(p.foo, "hallo".to_string());
        assert_eq!(p.bar, 9);
    }
}
