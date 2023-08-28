use std::collections::HashMap;

use sealedstruct::{ValidationError, ValidationResultExtensions};

use crate::{device::DeviceId, Recipe};

// Todo: Remove pub of fields
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, sealedstruct::Seal)]
pub struct DuplicateRecipeRaw {
    // OriginalId -> DuplicateId
    pub mappings: HashMap<DeviceId, DeviceId>,
    pub recipe: Recipe,
}

impl sealedstruct::Validator for DuplicateRecipeRaw {
    fn check(&self) -> sealedstruct::Result<()> {
        let mut result = Ok(());
        if self.mappings.len() != self.recipe.count_devices() {
            result = result.append_error(ValidationError::new(format!(
                "Mappings({}) != recipe({})",
                self.mappings.len(),
                self.recipe.count_devices()
            )))
        }
        let missing_ids = self
            .mappings
            .values()
            .filter(|id| !self.recipe.has_device(id))
            .collect::<Vec<_>>();
        if !missing_ids.is_empty() {
            result = result.append_error(ValidationError::new(format!(
                "Recipe is missing the following ids:
                 {missing_ids:?}"
            )))
        }
        result
    }
}

impl DuplicateRecipe {
    pub fn new_unwrap(mappings: HashMap<DeviceId, DeviceId>, recipe: Recipe) -> Self {
        DuplicateRecipe::new_unchecked(DuplicateRecipeRaw { mappings, recipe })
    }
}
