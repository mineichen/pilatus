use std::borrow::Borrow;
use std::fmt::Debug;
use std::io::{self, BufWriter, Read};
use std::path::Path;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tracing::trace;

use crate::{device::DeviceId, DeviceConfig, Name, RecipeId};
use crate::{
    RecipeAlreadyExistsError, TransactionError, UnknownRecipeError,
    UntypedDeviceParamsWithVariables,
};

use super::duplicate_recipe::DuplicateRecipe;
use super::recipe::Recipe;
use super::variable::{Variables, VariablesPatch};

// Ensures Recipes to be unique and that there is always an active recipe
// The uncommitted Recipe is stored in `all` to allow changes via id to affect the temporary Recipe
#[derive(Debug, Clone, Serialize)]
pub struct Recipes {
    active_id: RecipeId,
    // used to check for changes/restore
    active_backup: Recipe,
    all: IndexMap<RecipeId, Recipe>,
    variables: Variables,
}

impl<'de> Deserialize<'de> for Recipes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct DeserializeRecipes {
            active_id: RecipeId,
            active_backup: Recipe,
            all: IndexMap<RecipeId, Recipe>,
            variables: Variables,
        }

        let raw = DeserializeRecipes::deserialize(deserializer)?;

        if !raw.all.contains_key(&raw.active_id) {
            return Err(<D::Error as serde::de::Error>::custom(format_args!(
                "Unknown RecipeId {}",
                raw.active_id
            )));
        }

        Ok(Recipes {
            active_id: raw.active_id,
            active_backup: raw.active_backup,
            all: raw.all,
            variables: raw.variables,
        })
    }
}

impl Default for Recipes {
    fn default() -> Self {
        let id = RecipeId::default();
        let active = Recipe::default();
        Self {
            active_id: id.clone(),
            active_backup: active.clone(),
            all: IndexMap::from([(id, active)]),
            variables: Default::default(),
        }
    }
}

impl AsRef<Variables> for Recipes {
    fn as_ref(&self) -> &Variables {
        &self.variables
    }
}

impl AsMut<Variables> for Recipes {
    fn as_mut(&mut self) -> &mut Variables {
        &mut self.variables
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Duplicate name '{0}'")]
pub struct DuplicateNameError(Name);

pub struct ListActiveRecipesItem<'a> {
    pub id: DeviceId,
    pub running: &'a DeviceConfig,
    pub backup: &'a DeviceConfig,
}

#[allow(dead_code)]
impl Recipes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn commit_active(&mut self) {
        self.active_backup = self.active().1.clone();
    }

    /// Discards all uncommitted changes by replacing the running recipe with the backup.
    /// Tags are no uncommitted changes, so the current tags are kept.
    pub fn restore_active(&mut self) {
        let running = self
            .all
            .get_mut(&self.active_id)
            .expect("Active must always exist");
        let tags = std::mem::take(&mut running.tags);
        *running = self.active_backup.clone();
        running.tags = tags;
    }

    pub fn active_backup(&self) -> &Recipe {
        &self.active_backup
    }

    pub fn iter_running_join_backup(
        &self,
    ) -> impl Iterator<Item = Result<ListActiveRecipesItem<'_>, UncommittedChangesError>> {
        self.active_without_id()
            .iter()
            .map(|(running_id, running)| {
                let backup = self
                    .active_backup
                    .device_by_id(running_id)
                    .map_err(|_| UncommittedChangesError)?;
                Ok(ListActiveRecipesItem {
                    id: running_id,
                    running,
                    backup,
                })
            })
    }

    pub fn build_duplicate(
        &self,
        old_id: RecipeId,
        recipe: &Recipe,
    ) -> (RecipeId, DuplicateRecipe) {
        (self.get_unique_id(old_id), recipe.duplicate())
    }

    pub fn iter_without_backup(&self) -> impl Iterator<Item = (&'_ RecipeId, &'_ Recipe)> {
        self.all.iter()
    }

    pub fn iter_with_backup(&self) -> impl Iterator<Item = (&'_ RecipeId, &'_ Recipe)> {
        [(&self.active_id, &self.active_backup)]
            .into_iter()
            .chain(self.all.iter())
    }

    pub fn find_variable_usage_in_all_recipes<'a: 'b, 'b>(
        &'a self,
        vars: &'b VariablesPatch,
    ) -> impl Iterator<
        Item = (
            RecipeId,
            String,
            DeviceId,
            &'a UntypedDeviceParamsWithVariables,
        ),
    > + 'b {
        self.iter_with_backup().flat_map(|(id, recipe)| {
            recipe.iter().filter_map(|(device_id, device)| {
                device
                    .params
                    .variables_names()
                    .any(|v| vars.contains_key(&v))
                    .then_some((
                        id.clone(),
                        device.device_type.to_owned(),
                        device_id,
                        &device.params,
                    ))
            })
        })
    }

    pub fn recipeid_per_deviceid(&self) -> impl Iterator<Item = (DeviceId, RecipeId)> + '_ {
        self.iter_with_backup()
            .flat_map(|(rid, v)| v.iter().map(move |(id, _)| (id, rid.clone())))
    }

    pub fn has_uncommitted_changes(&self, id: &RecipeId) -> bool {
        &self.active_id == id && self.has_active_changes()
    }

    /// Tags are no uncommitted changes. The id of the active recipe can't change at all
    pub fn has_active_changes(&self) -> bool {
        let running = self.all.get(&self.active_id).expect("Must always exist");
        if running.count_devices() != self.active_backup.count_devices() {
            return true;
        }
        self.active_backup
            .iter()
            .zip(running.iter())
            .any(|(a, b)| a != b)
    }

    pub fn set_active(&mut self, id: &RecipeId) -> Result<Vec<DeviceId>, SetActiveError> {
        if let Some(recipe) = self.get_with_id(id) {
            if self.has_active_changes() {
                Err((UncommittedChangesError).into())
            } else {
                let ids = recipe.iter().map(|(id, _)| id).collect::<Vec<_>>();
                self.active_backup = recipe.clone();
                self.active_id = id.clone();
                Ok(ids)
            }
        } else {
            Err(UnknownRecipeError(id.clone()).into())
        }
    }

    pub fn has_device_on_running(&self, id: impl Borrow<DeviceId>) -> bool {
        self.active().1.has_device(id.borrow())
    }

    pub fn active(&self) -> (RecipeId, &Recipe) {
        (self.active_id.clone(), self.active_without_id())
    }
    fn active_without_id(&self) -> &Recipe {
        self.all
            .get(&self.active_id)
            .expect("Active must always exist")
    }

    pub fn get_active(&mut self) -> (RecipeId, &mut Recipe) {
        let id = &self.active_id;
        (
            id.clone(),
            self.get_with_id_mut(id.clone())
                .expect("Active must always exist"),
        )
    }

    pub fn update_recipe_id(
        &mut self,
        old_id: &RecipeId,
        new_id: RecipeId,
    ) -> Result<(), TransactionError> {
        if &self.active_id == old_id {
            return Err(TransactionError::Other(anyhow::anyhow!(
                "The id of the active recipe '{old_id}' cannot be changed"
            )));
        }
        if self.has_recipe(&new_id) {
            return Err(RecipeAlreadyExistsError(new_id).into());
        }

        let recipe = self
            .all
            .shift_remove(old_id)
            .ok_or_else(|| UnknownRecipeError(old_id.clone()))?;
        self.all.insert(new_id, recipe);
        Ok(())
    }

    pub fn get_with_id(&self, id: impl Borrow<RecipeId>) -> Option<&Recipe> {
        self.all.get(id.borrow())
    }

    pub fn get_with_id_mut(&mut self, id: impl Borrow<RecipeId>) -> Option<&mut Recipe> {
        self.all.get_mut(id.borrow())
    }

    pub fn get_device(&self, device_id: DeviceId) -> Option<&DeviceConfig> {
        self.all
            .values()
            .filter_map(|r| r.device_by_id(device_id).ok())
            .next()
    }

    pub fn has_recipe(&self, id: &RecipeId) -> bool {
        self.all.contains_key(id)
    }

    pub fn add_new(&mut self, new_recipe: Recipe) -> RecipeId {
        let id = self.get_unique_id(RecipeId::default());
        self.all.insert(id.clone(), new_recipe);
        id
    }

    pub fn add_inexistent(&mut self, new_recipe_id: RecipeId, new_recipe: Recipe) {
        assert!(self.all.insert(new_recipe_id, new_recipe).is_none());
    }

    pub fn try_add(&mut self, id: RecipeId, new_recipe: Recipe) -> Result<(), Recipe> {
        if let Some(x) = self.all.insert(id.clone(), new_recipe) {
            Err(self.all.insert(id, x).unwrap())
        } else {
            Ok(())
        }
    }
    pub fn remove(&mut self, id: &RecipeId) -> Result<Recipe, RemoveRecipeError> {
        if id == &self.get_active().0 {
            Err(RemoveRecipeError::IsActive)
        } else {
            self.all
                .shift_remove(id)
                .ok_or_else(|| UnknownRecipeError(id.clone()).into())
        }
    }

    pub fn new_with_recipe(r: Recipe) -> Self {
        let id = RecipeId::default();
        Recipes {
            active_id: id.clone(),
            active_backup: r.clone(),
            all: IndexMap::from([(id, r)]),
            variables: Default::default(),
        }
    }

    pub fn from_reader(r: impl Read) -> Result<Self, serde_json::Error> {
        serde_json::from_reader(r)
    }

    pub fn store_sync(&self, p: impl AsRef<Path> + Debug) -> Result<(), io::Error> {
        trace!(path = ?p, "storing json (sync)");
        let file = std::fs::File::create(p)?;
        let buf_write = BufWriter::new(file);
        Ok(serde_json::to_writer_pretty(buf_write, self)?)
    }

    fn get_unique_id(&self, mut id: RecipeId) -> RecipeId {
        let mut suggestions = id.suggest_unique();
        loop {
            if self.has_recipe(&id) {
                id = suggestions.next().expect("Suggests endless");
            } else {
                break id;
            }
        }
    }
}
#[derive(thiserror::Error, Debug)]
#[error("Uncommitted changes on active recipe")]
pub struct UncommittedChangesError;

impl From<UncommittedChangesError> for TransactionError {
    /// Keeps the type, so it can be detected with `anyhow::Error::is::<UncommittedChangesError>()`
    fn from(e: UncommittedChangesError) -> Self {
        TransactionError::Other(e.into())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SetActiveError {
    #[error(transparent)]
    UncommittedChanges(#[from] UncommittedChangesError),

    #[error(transparent)]
    UnknownRecipe(#[from] UnknownRecipeError),
}

impl From<SetActiveError> for TransactionError {
    fn from(value: SetActiveError) -> Self {
        match value {
            SetActiveError::UncommittedChanges(x) => x.into(),
            SetActiveError::UnknownRecipe(x) => x.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RemoveRecipeError {
    #[error("Cannot delete active recipe")]
    IsActive,
    #[error(transparent)]
    UnknownRecipe(#[from] UnknownRecipeError),
}

impl From<RemoveRecipeError> for TransactionError {
    fn from(value: RemoveRecipeError) -> Self {
        match value {
            RemoveRecipeError::IsActive => TransactionError::Other(anyhow::anyhow!("{value}")),
            RemoveRecipeError::UnknownRecipe(id) => TransactionError::UnknownRecipeId(id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{ops::Deref, sync::Arc};

    #[test]
    fn test_update_recipe_id() {
        let mut recipes = Recipes::new();
        let old_id = recipes.add_new(Recipe::default());
        let new_id = old_id
            .suggest_unique()
            .find(|id| !recipes.has_recipe(id))
            .unwrap();
        recipes.update_recipe_id(&old_id, new_id.clone()).unwrap();
        assert!(recipes.has_recipe(&new_id));
        assert!(!recipes.has_recipe(&old_id));
    }

    #[test]
    fn set_active_with_uncommitted_changes_fails() {
        let mut recipes = Recipes::new();
        let other_id = recipes.add_new(Recipe::default());
        let (_, r) = recipes.get_active();
        r.add_device(DeviceConfig::mock("param"));
        assert!(recipes.set_active(&other_id).is_err());
    }

    #[test]
    fn id_of_active_recipe_cannot_change() {
        let mut recipes = Recipes::new();
        let (active_id, _) = recipes.get_active();
        let new_id = active_id.suggest_unique().next().unwrap();
        let error = recipes
            .update_recipe_id(&active_id, new_id.clone())
            .unwrap_err();
        assert!(format!("{error}").contains("cannot be changed"), "{error}");
        assert_eq!(active_id, recipes.active().0);
        assert!(!recipes.has_recipe(&new_id));
    }

    #[test]
    fn tags_are_no_uncommitted_changes_and_kept_on_restore() {
        let mut recipes = Recipes::new();
        let tags = vec![Name::new("foo").unwrap()];
        let (_, r) = recipes.get_active();
        r.tags = tags.clone();
        assert!(!recipes.has_active_changes());

        let (_, r) = recipes.get_active();
        r.add_device(DeviceConfig::mock("param"));
        assert!(recipes.has_active_changes());

        recipes.restore_active();
        assert!(!recipes.has_active_changes());
        assert_eq!(0, recipes.active().1.count_devices());
        assert_eq!(tags, recipes.active().1.tags);
    }

    #[test]
    fn uncommitted_changes_error_is_detectable() {
        let TransactionError::Other(e) = TransactionError::from(UncommittedChangesError) else {
            panic!("Expected Other");
        };
        assert!(e.is::<UncommittedChangesError>());
    }

    #[test]
    fn test_update_recipe_id_with_existing_id() {
        let mut recipes = Recipes::new();
        let old_id = recipes.add_new(Recipe::default());
        let new_id = RecipeId::default();
        let error = recipes
            .update_recipe_id(&old_id, new_id.clone())
            .unwrap_err();
        let err = format!("{error}");
        assert!(
            err.contains(&format!("Recipe {new_id} already exists")),
            "{err}"
        );
    }

    #[test]
    fn deserialize_unknown_active() {
        let mut recipes = Recipes::new();
        recipes.add_new(Recipe::default());
        let mut json = serde_json::to_value(recipes).unwrap();
        json["active_id"] = serde_json::Value::String("some_recipe_id".to_string());
        let error = serde_json::from_value::<Recipes>(json).unwrap_err();
        assert!(format!("{error}").contains("Unknown RecipeId"), "{error}");
    }

    #[test]
    fn assign_unique_name_increments_on_existing_suffix() {
        let mut recipes = Recipes::new();
        let id = RecipeId::default().suggest_unique().next().unwrap(); // default_1
        recipes.add_inexistent(id.clone(), Recipe::default());

        assert_eq!(
            &Name::new("default_2").unwrap(),
            (Arc::<Name>::from(recipes.get_unique_id(id))).deref()
        );
    }

    #[test]
    fn assign_unique_name_with_lodash_but_no_number() {
        let mut recipes = Recipes::new();
        let id: RecipeId = serde_json::from_value(json!("default_test")).unwrap(); // default_1
        recipes.add_inexistent(id.clone(), Recipe::default());

        assert_eq!(
            &Name::new("default_test_1").unwrap(),
            (Arc::<Name>::from(recipes.get_unique_id(id))).deref()
        );
    }
}
