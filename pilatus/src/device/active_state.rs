use crate::Recipes;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ActiveState {
    /// Should contain information like DeviceState (Device will be mapped)
    #[serde(flatten)]
    recipes: Recipes,
    has_uncommitted_changes: bool,
}

impl ActiveState {
    pub fn new(recipes: Recipes, has_uncommitted_changes: bool) -> Self {
        Self {
            recipes,
            has_uncommitted_changes,
        }
    }

    pub fn recipes(&self) -> &Recipes {
        &self.recipes
    }
    pub fn has_uncommitted_changes(&self) -> bool {
        self.has_uncommitted_changes
    }
}
