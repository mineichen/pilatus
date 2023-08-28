use std::borrow::Cow;

use pilatus::{
    device::{ActorErrorUnknownDevice, DeviceContext, DeviceId, InfallibleParamApplier},
    DeviceConfig, Recipe, RecipeId, Recipes, TransactionError, Variables,
};

use super::actions::DeviceActions;

// Returns the new Recipe if it didn't work
pub(super) async fn recipes_try_add_new_with_id(
    recipes: &mut Recipes,
    id: RecipeId,
    mut new_recipe: Recipe,
    device_actions: &dyn DeviceActions,
) -> Result<(), Recipe> {
    let vars: &Variables = recipes.as_ref();
    let mut iter = new_recipe.devices.iter_unordered_mut();
    while let Some((&id, device)) = iter.next() {
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

            Err(_) => {
                drop(iter);
                return Err(new_recipe);
            }
        };
    }
    drop(iter);
    recipes.try_add(id, new_recipe)?;
    Ok(())
}
const NO_RECIPE_WITH_DEVICE_ID: Cow<str> = Cow::Borrowed("There is not such device in any recipe");

pub(super) trait RecipesExt {
    fn get_with_id_or_error(&self, id: &RecipeId) -> Result<&Recipe, TransactionError>;
    fn get_with_id_or_error_mut(&mut self, id: &RecipeId) -> Result<&mut Recipe, TransactionError>;
    fn get_device_or_error(
        &self,
        device_id: DeviceId,
    ) -> Result<&DeviceConfig, ActorErrorUnknownDevice>;
}

impl RecipesExt for Recipes {
    fn get_with_id_or_error(&self, id: &RecipeId) -> Result<&Recipe, TransactionError> {
        self.get_with_id(id)
            .ok_or(TransactionError::UnknownRecipeId(id.clone()))
    }
    fn get_with_id_or_error_mut(&mut self, id: &RecipeId) -> Result<&mut Recipe, TransactionError> {
        self.get_with_id_mut(id)
            .ok_or(TransactionError::UnknownRecipeId(id.clone()))
    }
    fn get_device_or_error(
        &self,
        device_id: DeviceId,
    ) -> Result<&DeviceConfig, ActorErrorUnknownDevice> {
        self.get_device(device_id).ok_or(ActorErrorUnknownDevice {
            device_id,
            detail: NO_RECIPE_WITH_DEVICE_ID,
        })
    }
}
