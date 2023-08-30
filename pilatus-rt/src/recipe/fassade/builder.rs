use std::{path::PathBuf, sync::Arc};

use super::RecipeServiceFassade;
use crate::recipe::{actions::DeviceActions, ChangeParamsStrategy, RecipeServiceBuilder};

pub struct RecipeServiceFassadeBuilder {
    pub recipe_builder: RecipeServiceBuilder,
}

impl RecipeServiceFassadeBuilder {
    pub fn new(path: impl Into<PathBuf>, device_actions: Arc<dyn DeviceActions>) -> Self {
        RecipeServiceFassadeBuilder {
            recipe_builder: RecipeServiceBuilder::new(path, device_actions),
        }
    }
    pub fn with_change_strategy(mut self, s: ChangeParamsStrategy) -> RecipeServiceFassadeBuilder {
        self.recipe_builder = self.recipe_builder.with_change_strategy(s);
        self
    }

    pub fn replace_permissioner(
        mut self,
        s: Arc<dyn DeviceActions>,
    ) -> RecipeServiceFassadeBuilder {
        self.recipe_builder = self.recipe_builder.replace_permissioner(s);
        self
    }

    pub fn build(self) -> RecipeServiceFassade {
        RecipeServiceFassade {
            recipe_service: Arc::new(self.recipe_builder.build()),
        }
    }
}
