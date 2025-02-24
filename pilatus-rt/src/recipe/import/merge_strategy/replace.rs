use std::{collections::HashSet, path::Path};

use futures::{future::BoxFuture, FutureExt};
use pilatus::{device::DeviceId, AlreadyExistsError, IrreversibleError, Recipe, RecipeId};

use crate::recipe::recipes::recipes_try_add_new_with_id;

use super::MergeStrategyContext;

pub(in super::super) struct Replace {
    delete_devices: HashSet<DeviceId>,
}

impl Replace {
    pub fn new() -> Self {
        Self {
            delete_devices: Default::default(),
        }
    }
}
impl super::MergeStrategy for Replace {
    fn handle_json<'a>(
        &'a mut self,
        ctx: MergeStrategyContext<'a>,
        new_id: RecipeId,
        recipe: Recipe,
    ) -> BoxFuture<'a, Result<(), AlreadyExistsError>> {
        async move {
            if let Ok(x) = ctx.recipes_copy.remove(&new_id) {
                self.delete_devices
                    .extend(x.devices.iter().map(|(id, _)| id));
            }
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
        recipe_root: &'a Path,
        _tmp_root: &'a Path,
    ) -> BoxFuture<'a, Result<(), IrreversibleError>> {
        async {
            assert!(
                recipe_root.is_relative() || recipe_root.iter().count() > 1,
                "Don't update root!"
            );
            for x in self.delete_devices.iter().filter_map(|x| {
                let r = recipe_root.join(x.to_string());
                r.exists().then_some(r)
            }) {
                tokio::fs::remove_dir_all(x).await?;
            }
            Ok(())
        }
        .boxed()
    }
}
