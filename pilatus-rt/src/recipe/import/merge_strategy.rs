use std::path::Path;

use futures::future::BoxFuture;

use pilatus::{IrreversibleError, Recipe, RecipeAlreadyExistsError, RecipeId, Recipes};

mod duplicate;
mod replace;
mod unspecified;

pub(super) use duplicate::Duplicate;
pub(super) use replace::Replace;
pub(super) use unspecified::Unspecified;

use crate::recipe::actions::DeviceActions;

pub(super) struct MergeStrategyContext<'a> {
    pub recipes_copy: &'a mut Recipes,
    pub device_actions: &'a dyn DeviceActions,
}

pub(super) trait MergeStrategy: 'static + Send {
    fn handle_json<'a>(
        &'a mut self,
        ctx: MergeStrategyContext<'a>,
        new_id: RecipeId,
        recipe: Recipe,
    ) -> BoxFuture<'a, Result<(), RecipeAlreadyExistsError>>;
    fn finalize<'a>(
        &'a mut self,
        recipe_root: &'a Path,
        tmp_root: &'a Path,
    ) -> BoxFuture<'a, Result<(), IrreversibleError>>;
}
