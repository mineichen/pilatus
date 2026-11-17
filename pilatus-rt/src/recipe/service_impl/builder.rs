#[cfg(any(test, feature = "unstable"))]
use std::{path::PathBuf, sync::Arc};

#[cfg(any(test, feature = "unstable"))]
use super::RecipeServiceImpl;
#[cfg(any(test, feature = "unstable"))]
use crate::recipe::{actions::DeviceActions, ChangeParamsStrategy, RecipeServiceBuilder};

#[cfg(any(test, feature = "unstable"))]
pub struct RecipeServiceImplBuilder {
    pub recipe_builder: RecipeServiceBuilder,
}

#[cfg(any(test, feature = "unstable"))]
#[deprecated(note = "Renamed to `RecipeServiceImplBuilder`")]
pub type RecipeServiceFassadeBuilder = RecipeServiceImplBuilder;

#[cfg(any(test, feature = "unstable"))]
impl RecipeServiceImplBuilder {
    pub fn new(path: impl Into<PathBuf>, device_actions: Arc<dyn DeviceActions>) -> Self {
        RecipeServiceImplBuilder {
            recipe_builder: RecipeServiceBuilder::new(path, device_actions),
        }
    }
    pub fn with_change_strategy(mut self, s: ChangeParamsStrategy) -> RecipeServiceImplBuilder {
        self.recipe_builder = self.recipe_builder.with_change_strategy(s);
        self
    }

    pub fn replace_permissioner(mut self, s: Arc<dyn DeviceActions>) -> RecipeServiceImplBuilder {
        self.recipe_builder = self.recipe_builder.replace_permissioner(s);
        self
    }

    pub fn build(self) -> RecipeServiceImpl {
        RecipeServiceImpl {
            recipe_service: Arc::new(self.recipe_builder.build()),
            runner: None,
        }
    }
}
