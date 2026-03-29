#[cfg(any(test, feature = "unstable"))]
use std::{path::PathBuf, sync::Arc};

#[cfg(any(test, feature = "unstable"))]
use super::RecipeServiceFassade;
#[cfg(any(test, feature = "unstable"))]
use crate::recipe::{actions::DeviceActions, ChangeParamsStrategy, RecipeServiceBuilder};

#[cfg(any(test, feature = "unstable"))]
pub struct RecipeServiceFassadeBuilder {
    pub recipe_builder: RecipeServiceBuilder,
}

#[cfg(any(test, feature = "unstable"))]
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
