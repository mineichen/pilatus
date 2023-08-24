use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
};

use chrono::{DateTime, Utc};
use sealedstruct::{Seal, ValidationResultExtensions, Validator};
use serde::{Deserialize, Serialize};

use super::{device_config::DeviceConfig, ord_hash_map::OrdHashMap};
use crate::{
    device::{ActorErrorUnknownDevice, DeviceId},
    Name, RecipeId, UntypedDeviceParamsWithVariables,
};

#[derive(Debug, PartialEq, Eq, Clone, Deserialize, Seal)]
#[serde(deny_unknown_fields)]
pub struct RecipeMetadataRaw {
    pub new_id: RecipeId,
    pub tags: Vec<Name>,
}

#[derive(Debug, thiserror::Error)]
#[error("Device with {0} exists already")]
pub struct DeviceWithSameIdExists(DeviceId);

impl Validator for RecipeMetadataRaw {
    fn check(&self) -> sealedstruct::Result<()> {
        let mut tags = HashSet::new();
        let mut errors: sealedstruct::Result<()> = Ok(());
        for tag in self.tags.iter() {
            if !tags.insert(tag) {
                errors = errors.append_error(sealedstruct::ValidationError::new(format!(
                    "Tag {} appears multiple times",
                    tag as &str
                )))
            }
        }

        RecipeMetadataResult {
            new_id: Ok(()),
            tags: errors,
        }
        .into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub created: DateTime<Utc>,
    pub tags: Vec<Name>,
    pub devices: OrdHashMap<DeviceId, DeviceConfig>,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            created: Utc::now(),
            tags: Default::default(),
            devices: Default::default(),
        }
    }
}

impl Recipe {
    /// This method replaces Uuids in the DeviceConfig too, so all links should still work
    pub fn duplicate(&self) -> (Self, HashMap<DeviceId, DeviceId>) {
        let mapped = self
            .devices
            .keys()
            .map(|&id| (id, DeviceId::new_v4()))
            .collect::<HashMap<_, _>>();
        let mut config = serde_json::to_string(self).expect("Always works");
        for (old_id, new_id) in mapped.iter() {
            config = config.replace(&format!("\"{old_id}\""), &format!("\"{new_id}\""));
        }
        (serde_json::from_str(&config).expect("Valid json"), mapped)
    }

    pub fn has_device(&self, id: &DeviceId) -> bool {
        self.devices.contains_key(id)
    }

    pub fn device_by_id(&self, id: DeviceId) -> Result<&DeviceConfig, ActorErrorUnknownDevice> {
        self.devices.get(&id).ok_or(ActorErrorUnknownDevice {
            device_id: id,
            detail: Cow::Owned(format!("Recipe doesn't contain a device with id {id}")),
        })
    }

    pub fn get_device_by_id(
        &mut self,
        id: DeviceId,
    ) -> Result<&mut DeviceConfig, ActorErrorUnknownDevice> {
        self.devices.get_mut(&id).ok_or(ActorErrorUnknownDevice {
            device_id: id,
            detail: Cow::Owned(format!("Recipe doesn't contain a device with id {id}")),
        })
    }

    pub fn add_device(&mut self, device: DeviceConfig) -> DeviceId {
        let id = DeviceId::new_v4();
        self.devices.insert(id, device);
        id
    }

    pub fn add_device_with_id(
        &mut self,
        id: DeviceId,
        device: DeviceConfig,
    ) -> Result<(), DeviceWithSameIdExists> {
        if let Some(x) = self.devices.insert(id, device) {
            self.devices.insert(id, x).expect("Must exist");
            Err(DeviceWithSameIdExists(id))
        } else {
            Ok(())
        }
    }

    pub fn update_device_params_committed(
        &mut self,
        id: DeviceId,
        params: UntypedDeviceParamsWithVariables,
    ) -> Result<(), ActorErrorUnknownDevice> {
        self.get_mut_device(id)?.update_params_committed(params);
        Ok(())
    }

    pub fn update_device_params_uncommitted(
        &mut self,
        id: DeviceId,
        params: UntypedDeviceParamsWithVariables,
    ) -> Result<(), ActorErrorUnknownDevice> {
        self.get_mut_device(id)?.update_params_uncommitted(params);
        Ok(())
    }

    fn get_mut_device(
        &mut self,
        device_id: DeviceId,
    ) -> Result<&mut DeviceConfig, ActorErrorUnknownDevice> {
        self.devices
            .get_mut(&device_id)
            .ok_or(ActorErrorUnknownDevice {
                device_id,
                detail: Cow::Borrowed("No device with this ID in any recipe"),
            })
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_new_recipe_has_no_devices() {
        assert_eq!(0, Recipe::default().devices.len());
    }

    #[test]
    fn test_get_device_by_id() {
        let device = DeviceConfig::mock("Test");
        let mut recipe = Recipe::default();
        let id = recipe.add_device(device.clone());

        let d = recipe.get_device_by_id(id).unwrap();
        assert_eq!(device, d.to_owned());
        let eid = DeviceId::new_v4();

        assert_eq!(
            Err(ActorErrorUnknownDevice {
                device_id: eid,
                detail: Cow::Owned(format!("Recipe doesn't contain a device with id {eid}")),
            }),
            recipe.get_device_by_id(eid)
        );
    }

    #[test]
    fn recipe_add_device_has_one_devices_afterwards() {
        let device = DeviceConfig::mock("Test");
        let mut recipe = Recipe::default();
        let _id = recipe.add_device(device);
        assert_eq!(1, recipe.devices.len());
    }
}
