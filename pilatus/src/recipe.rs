mod device;
mod device_config;
mod duplicate_recipe;
mod error;
mod file;
#[allow(clippy::module_inception)]
mod recipe;
mod recipes;
mod service;
mod variable;

pub use device::*;
pub use device_config::DeviceConfig;
pub use duplicate_recipe::*;
pub use error::*;
pub use file::*;
pub use recipe::*;
pub use recipes::*;
use serde::{de::Error, Deserialize, Serialize};
pub use service::*;

pub use variable::*;

crate::name::name_wrapper::wrapped_name!(RecipeId);
impl std::default::Default for RecipeId {
    fn default() -> Self {
        Self(std::sync::Arc::new(crate::Name::new("default").unwrap()))
    }
}

/// Every Object with a key __var is guaranteed not to contain any other key and a string as value
/// If UntypedDeviceParamsWithVariables is constructible anyway, this is a bug.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
pub struct UntypedDeviceParamsWithVariables(serde_json::Value);
pub use UntypedDeviceParamsWithoutVariables;

#[derive(Deserialize, Serialize)]
pub struct ParameterUpdate {
    pub parameters: UntypedDeviceParamsWithVariables,
    pub variables: VariablesPatch,
}

pub struct InitRecipeListener(Box<dyn Fn(&mut Recipe) + Send + Sync>);

impl InitRecipeListener {
    pub fn new(i: impl Fn(&mut Recipe) + Send + Sync + 'static) -> Self {
        Self(Box::new(i) as _)
    }
    pub fn call(&self, x: &mut Recipe) {
        (self.0)(x)
    }
}

impl std::ops::Deref for UntypedDeviceParamsWithVariables {
    type Target = serde_json::Value;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for UntypedDeviceParamsWithVariables {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl UntypedDeviceParamsWithVariables {
    #[cfg(any(test, feature = "unstable"))]
    pub fn new(inner: serde_json::Value) -> Self {
        Self(inner)
    }
    #[cfg(not(any(test, feature = "unstable")))]
    fn new(value: serde_json::Value) -> Self {
        Self(value)
    }
    pub fn variables_names(&self) -> impl Iterator<Item = String> {
        let mut result = Default::default();
        Self::add_variable_names(&self.0, &mut result);
        result.into_iter()
    }
    fn add_variable_names(value: &serde_json::Value, found: &mut smallvec::SmallVec<[String; 8]>) {
        match value {
            serde_json::Value::Array(list) => {
                list.iter().for_each(|x| Self::add_variable_names(x, found))
            }
            serde_json::Value::Object(o) => {
                if let Some(serde_json::Value::String(x)) = o.get(JSON_VAR_KEYWORD) {
                    found.push(x.clone());
                } else {
                    o.values().for_each(|x| Self::add_variable_names(x, found))
                }
            }
            _ => {}
        }
    }

    pub fn from_serializable(serializable: impl Serialize) -> serde_json::Result<Self> {
        let inner = serde_json::to_value(serializable)?;

        check_recursive(&inner).map_err(|e| serde_json::Error::custom(e))?;
        Ok(Self(inner))
    }
}
// Makes sure that `serializable` doesn't contain invalid variable declarations (multiple fields or str-len = 0)
fn check_recursive(v: &serde_json::Value) -> Result<(), &serde_json::Value> {
    match v {
        serde_json::Value::Array(x) => x.iter().try_for_each(check_recursive),
        serde_json::Value::Object(x) => {
            if let Some(var_name) = x.get(JSON_VAR_KEYWORD) {
                if x.len() > 1 || !matches!(var_name, serde_json::Value::String(_)) {
                    Err(v)
                } else {
                    Ok(())
                }
            } else {
                x.values().try_for_each(check_recursive)
            }
        }
        _ => Ok(()),
    }
}

impl<'de> Deserialize<'de> for UntypedDeviceParamsWithVariables {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        serde_json::Value::deserialize(deserializer).and_then(|r| match check_recursive(&r) {
            Ok(_) => Ok(Self(r)),
            Err(e) => Err(<D::Error as serde::de::Error>::custom(e)),
        })
    }
}
