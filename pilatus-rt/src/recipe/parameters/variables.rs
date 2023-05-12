use std::sync::Arc;

use minfac::{Registered, ServiceCollection};
use tokio::sync::Mutex;

use pilatus::{
    Recipes, UntypedDeviceParamsWithVariables, UntypedDeviceParamsWithoutVariables,
    UpdateParamsMessageError,
};

use crate::recipe::RecipeServiceImpl;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<Registered<Arc<RecipeServiceImpl>>>()
        .register(|r| VariableService(r.recipes.clone()));
}

#[derive(Debug, Clone, Default)]
pub struct VariableService(Arc<Mutex<Recipes>>);

impl VariableService {
    #[allow(dead_code)]
    pub async fn resolve(
        &self,
        t: &UntypedDeviceParamsWithVariables,
    ) -> Result<UntypedDeviceParamsWithoutVariables, UpdateParamsMessageError> {
        let lock = self.0.lock().await;
        lock.as_ref().resolve(t)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replace_nothing() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"text": "value", "number": 42}"#).unwrap();
        let store = VariableService::default();
        let a = store
            .resolve(&UntypedDeviceParamsWithVariables::new(json.clone()))
            .await
            .expect("Replace successful");
        assert_eq!(json, a.params_as::<serde_json::Value>().unwrap());
    }
}
