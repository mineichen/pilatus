use pilatus::{
    device::{DeviceContext, DeviceId, InfallibleParamApplier},
    DeviceConfig, Recipe, RecipeId, Recipes, TransactionError, UnknownDeviceError,
    UnknownRecipeError, Variables,
};

use super::actions::DeviceActions;

// Returns the new Recipe if it didn't work
pub(super) async fn recipes_try_add_new_with_id(
    recipes: &mut Recipes,
    id: RecipeId,
    mut new_recipe: Recipe,
    device_actions: &dyn DeviceActions,
) -> Result<(), (Recipe, TransactionError)> {
    let vars: &Variables = recipes.as_ref();
    for (&id, device) in new_recipe.devices.iter_mut() {
        match device_actions
            .validate(
                &device.device_type,
                DeviceContext::new(id, vars.clone(), device.params.clone()),
            )
            .await
        {
            Ok(changes) => {
                device.apply(changes).await;
            }

            Err(e) => {
                return Err((new_recipe, e));
            }
        };
    }
    recipes.try_add(id.clone(), new_recipe).map_err(|r| {
        (
            r,
            TransactionError::Other(anyhow::anyhow!("Recipe {id} already exists ")),
        )
    })
}

pub(super) trait RecipesExt {
    fn get_with_id_or_error(&self, id: &RecipeId) -> Result<&Recipe, UnknownRecipeError>;
    fn get_with_id_or_error_mut(
        &mut self,
        id: &RecipeId,
    ) -> Result<&mut Recipe, UnknownRecipeError>;
    fn get_device_or_error(&self, device_id: DeviceId)
        -> Result<&DeviceConfig, UnknownDeviceError>;
}

impl RecipesExt for Recipes {
    fn get_with_id_or_error(&self, id: &RecipeId) -> Result<&Recipe, UnknownRecipeError> {
        self.get_with_id(id).ok_or(UnknownRecipeError(id.clone()))
    }
    fn get_with_id_or_error_mut(
        &mut self,
        id: &RecipeId,
    ) -> Result<&mut Recipe, UnknownRecipeError> {
        self.get_with_id_mut(id)
            .ok_or(UnknownRecipeError(id.clone()))
    }
    fn get_device_or_error(
        &self,
        device_id: DeviceId,
    ) -> Result<&DeviceConfig, UnknownDeviceError> {
        self.get_device(device_id)
            .ok_or(UnknownDeviceError(device_id))
    }
}
