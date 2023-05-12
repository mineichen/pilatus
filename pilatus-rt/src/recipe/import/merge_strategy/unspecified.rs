use std::path::Path;

use futures::{future::BoxFuture, FutureExt};
use pilatus::{AlreadyExistsError, IrreversibleError, Recipe, RecipeId};

use crate::recipe::recipes::recipes_try_add_new_with_id;

use super::MergeStrategyContext;

pub(in super::super) struct Unspecified;
impl super::MergeStrategy for Unspecified {
    fn handle_json<'a>(
        &'a mut self,
        ctx: MergeStrategyContext<'a>,
        new_id: RecipeId,
        recipe: Recipe,
    ) -> BoxFuture<'a, Result<(), AlreadyExistsError>> {
        async move {
            recipes_try_add_new_with_id(
                ctx.recipes_copy,
                new_id.clone(),
                recipe,
                ctx.device_actions,
            )
            .await
            .map_err(|_| AlreadyExistsError(new_id))
        }
        .boxed()
    }
    fn finalize<'a>(
        &'a mut self,
        _recipe_root: &'a Path,
        _tmp_root: &'a Path,
    ) -> BoxFuture<'a, Result<(), IrreversibleError>> {
        async { Ok(()) }.boxed()
    }
}
