use std::sync::Arc;

use crate::recipe::{
    actions::DeviceActions, ChangeParamsStrategy, RecipeServiceBuilder, RecipeServiceImpl,
};

pub struct RecipeServiceFassade {
    pub(crate) recipe_service: Arc<RecipeServiceImpl>,
}

pub struct RecipeServiceFassadeBuilder {
    pub recipe_builder: RecipeServiceBuilder,
}

impl RecipeServiceFassadeBuilder {
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
