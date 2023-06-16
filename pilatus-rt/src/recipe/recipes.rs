use std::borrow::Cow;

use pilatus::{
    device::{ActorErrorUnknownDevice, DeviceContext, DeviceId},
    DeviceConfig, Recipe, RecipeId, Recipes, TransactionError, Variables,
};

use super::actions::DeviceActions;

// Returns the new Recipe if it didn't work
pub(super) async fn recipes_try_add_new_with_id(
    recipes: &mut Recipes,
    id: RecipeId,
    new_recipe: Recipe,
    device_actions: &dyn DeviceActions,
) -> Result<(), Recipe> {
    let is_err = 'block: {
        for (&id, device) in new_recipe.devices.iter() {
            let vars: &Variables = recipes.as_ref();
            let Ok(params) = vars.resolve(&device.params) else {
                break 'block true;
            };
            if device_actions
                .validate(&device.device_type, DeviceContext::new(id, params))
                .await
                .is_err()
            {
                break 'block true;
            };
        }
        false
    };
    if is_err {
        return Err(new_recipe);
    }

    recipes.try_add(id, new_recipe)
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
