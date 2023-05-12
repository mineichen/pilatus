use std::{collections::HashMap, path::Path};

use futures::{future::BoxFuture, FutureExt};
use pilatus::{device::DeviceId, AlreadyExistsError, IrreversibleError, Recipe, RecipeId};

use crate::recipe::recipes::recipes_try_add_new_with_id;

use super::MergeStrategyContext;

pub(in super::super) struct Duplicate {
    device_id_map: HashMap<DeviceId, DeviceId>,
}

impl Duplicate {
    pub fn new() -> Self {
        Self {
            device_id_map: Default::default(),
        }
    }
}
impl super::MergeStrategy for Duplicate {
    fn handle_json<'a>(
        &'a mut self,
        ctx: MergeStrategyContext<'a>,
        recipe_id: RecipeId,
        recipe: Recipe,
    ) -> BoxFuture<'a, Result<(), AlreadyExistsError>> {
        async move {
            let (insert_id, to_insert) = if ctx.recipes_copy.has_recipe(&recipe_id) {
                let (new_id, r, did_map) = ctx.recipes_copy.build_duplicate(recipe_id, &recipe);
                self.device_id_map.extend(did_map);

                (new_id, r)
            } else {
                (recipe_id, recipe)
            };

            recipes_try_add_new_with_id(
                ctx.recipes_copy,
                insert_id.clone(),
                to_insert,
                ctx.device_actions,
            )
            .await
            .map_err(|_| AlreadyExistsError(insert_id))
        }
        .boxed()
    }
    fn finalize<'a>(
        &'a mut self,
        _recipe_root: &'a Path,
        tmp_root: &'a Path,
    ) -> BoxFuture<'a, Result<(), IrreversibleError>> {
        async move {
            for (from, to) in self.device_id_map.iter() {
                let _ignore = tokio::fs::rename(
                    tmp_root.join(from.to_string()),
                    tmp_root.join(to.to_string()),
                )
                .await;
            }

            Ok(())
        }
        .boxed()
    }
}
