use std::collections::HashSet;
use std::fmt::Debug;
use std::io::{self};

use std::sync::Arc;

use async_trait::async_trait;
use futures_util::stream::BoxStream;

use serde::Deserialize;

use uuid::Uuid;

use crate::device::{ActiveState, DeviceId};
use crate::{
    EntryReader, EntryWriter, Name, ParameterUpdate, RecipeId, RecipeMetadata, TransactionError,
    UntypedDeviceParamsWithVariables, VariableConflict,
};

use super::recipe::{Recipe, UnknownDeviceError};

pub type RecipeExporter = Arc<dyn RecipeExporterTrait + Send + Sync>;
#[async_trait]
pub trait RecipeExporterTrait {
    async fn export<'a>(
        &self,
        recipe_id: RecipeId,
        mut writer: Box<dyn EntryWriter>,
    ) -> anyhow::Result<()>;
}

#[derive(Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub enum IntoMergeStrategy {
    #[default]
    Unspecified,
    Duplicate,
    Replace,
}

#[derive(Debug)]
pub struct ImportRecipesOptions {
    pub merge_strategy: IntoMergeStrategy,
    pub is_dry_run: bool,
}

impl Default for ImportRecipesOptions {
    fn default() -> Self {
        Self {
            merge_strategy: Default::default(),
            is_dry_run: true,
        }
    }
}

pub type RecipeImporter = Box<dyn RecipeImporterTrait + Send + Sync>;
#[async_trait]
pub trait RecipeImporterTrait {
    async fn import(
        &self,
        reader: &mut dyn EntryReader,
        options: ImportRecipesOptions,
    ) -> Result<(), ImportRecipeError>;
}

type BoxedImporter = Box<dyn ImporterTrait + Send + Sync>;

#[async_trait]
pub trait ImporterTrait: Debug {
    async fn close_async(self: Box<Self>) -> io::Result<()>;
    async fn apply(
        self: Box<Self>,
        merge_strategy: IntoMergeStrategy,
    ) -> Result<(), ImportRecipeError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ImportRecipeError {
    #[error("Invalid format")]
    InvalidFormat(anyhow::Error),
    #[error("IO: {0:?}")]
    Io(#[from] io::Error),

    /// This shouldn't happen with untampered recipes but only, if a recipe_id is manually changed. There is no plan needed to continue
    #[error(
        "File cannot be imported with any strategy, as {0:?} is contained in at least two recipes {1:?} and {2:?}"
    )]
    ExistingDeviceInOtherRecipe(DeviceId, RecipeId, RecipeId),

    #[error("Found conflicts: {0:?}")]
    Conflicts(HashSet<RecipeId>, Vec<VariableConflict>, BoxedImporter),

    #[error("Can't import recipe which is currently active")]
    ContainsActiveRecipe,

    #[error("{0:?}")]
    Irreversible(#[from] IrreversibleError),
}

#[derive(Debug, thiserror::Error)]
#[error("This is a bug in the program, as this should never happen. Please file a bug report to resolve this issue in the future: {0}")]
pub struct IrreversibleError(#[from] io::Error);

#[derive(Debug, thiserror::Error)]
#[error("{0:?} already exists")]
pub struct AlreadyExistsError(pub RecipeId);

pub type RecipeService = Arc<dyn RecipeServiceTrait + Send + Sync>;
#[async_trait]
pub trait RecipeServiceTrait {
    async fn add_new_default_recipe_with(
        &self,
        options: TransactionOptions,
    ) -> Result<(RecipeId, Recipe), TransactionError>;
    async fn update_recipe_metadata_with(
        &self,
        id: RecipeId,
        data: RecipeMetadata,
        options: TransactionOptions,
    ) -> Result<(), TransactionError>;

    async fn delete_recipe_with(
        &self,
        recipe_id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(), TransactionError>;
    async fn delete_recipe(&self, recipe_id: RecipeId) -> Result<(), TransactionError> {
        self.delete_recipe_with(recipe_id, Default::default()).await
    }

    async fn duplicate_recipe_with(
        &self,
        recipe_id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(RecipeId, Recipe), TransactionError>;
    async fn duplicate_recipe(
        &self,
        recipe_id: RecipeId,
    ) -> Result<(RecipeId, Recipe), TransactionError> {
        self.duplicate_recipe_with(recipe_id, Default::default())
            .await
    }

    async fn state(&self) -> ActiveState;

    async fn activate_recipe_with(
        &self,
        id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(), TransactionError>;
    async fn activate_recipe(&self, id: RecipeId) -> Result<(), TransactionError> {
        self.activate_recipe_with(id, Default::default()).await
    }

    async fn update_device_params_with(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        values: ParameterUpdate,
        options: TransactionOptions,
    ) -> Result<(), TransactionError>;
    async fn update_device_params(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        values: ParameterUpdate,
    ) -> Result<(), TransactionError> {
        self.update_device_params_with(recipe_id, device_id, values, Default::default())
            .await
    }

    async fn restore_active_with(&self, transaction_key: Uuid) -> Result<(), TransactionError>;
    async fn restore_active(&self) -> Result<(), TransactionError> {
        self.restore_active_with(Uuid::new_v4()).await
    }

    async fn commit_active_with(&self, transaction_key: Uuid) -> Result<(), TransactionError>;
    async fn commit_active(&self) -> Result<(), TransactionError> {
        self.commit_active_with(Uuid::new_v4()).await
    }

    async fn delete_device_with(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        options: TransactionOptions,
    ) -> Result<(), TransactionError>;
    async fn delete_device(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
    ) -> Result<(), TransactionError> {
        self.delete_device_with(recipe_id, device_id, Default::default())
            .await
    }

    // Before having uncommitted recipes, devices were able to be uncommitted
    // This feature became partially obsolete with uncommitted recipes feature.
    async fn restore_committed(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        transaction: Uuid,
    ) -> Result<(), TransactionError>;
    async fn update_device_name_with(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        name: Name,
        options: TransactionOptions,
    ) -> Result<(), TransactionError>;
    fn get_update_receiver(&self) -> BoxStream<'static, Uuid>;
}

#[derive(Deserialize, Clone)]
#[serde(default)]
#[non_exhaustive]
pub struct TransactionOptions {
    pub key: Uuid,
    pub committed: bool,
}

impl TransactionOptions {
    pub fn update_device_params(
        &self,
        recipe: &mut Recipe,
        device_id: DeviceId,
        new_params: UntypedDeviceParamsWithVariables,
    ) -> Result<(), UnknownDeviceError> {
        if self.committed {
            recipe.update_device_params_committed(device_id, new_params)
        } else {
            recipe.update_device_params_uncommitted(device_id, new_params)
        }
    }
}

impl Default for TransactionOptions {
    fn default() -> Self {
        Self {
            key: Uuid::new_v4(),
            committed: true,
        }
    }
}
