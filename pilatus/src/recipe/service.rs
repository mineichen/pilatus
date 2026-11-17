use std::collections::HashSet;
use std::fmt::Debug;
use std::io::{self};

use std::sync::Arc;

use async_trait::async_trait;
use futures_util::stream::BoxStream;

use serde::{Deserialize, Serialize};

use uuid::Uuid;

use crate::device::{ActiveState, DeviceId};
use crate::{
    EntryReader, EntryWriter, Name, ParameterUpdate, RecipeId, RecipeMetadata, TransactionError,
    UntypedDeviceParamsWithVariables, VariableConflict,
};

use super::recipe::{Recipe, UnknownDeviceError};

/// Decides what happens to uncommitted changes of the active recipe, when another recipe is started.
///
/// Discarding is deliberately only available in combination with a recipe change: All devices are stopped
/// at that point, so neither parameters need to be applied to running devices, nor are device files changed
/// while a device is using them.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingChangesStrategy {
    /// Fail, if there are uncommitted changes
    #[default]
    Deny,
    /// Drop uncommitted changes (parameters, devices and files) and restore the last committed state
    Discard,
    /// Commit the changes, before the recipe is changed
    Commit,
}

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
    #[error("Invalid format: {0}")]
    InvalidFormat(anyhow::Error),
    #[error(transparent)]
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

    #[error(transparent)]
    Irreversible(#[from] IrreversibleError),
}

#[derive(Debug, thiserror::Error)]
#[error("This is a bug in the program, as this should never happen. Please file a bug report to resolve this issue in the future: {0}")]
pub struct IrreversibleError(#[from] io::Error);

#[derive(Debug, thiserror::Error)]
#[error("Recipe {0} already exists")]
pub struct RecipeAlreadyExistsError(pub RecipeId);

/// Cheaply clonable handle to the `RecipeServiceFacade` of the running system.
///
/// The methods accept `impl Into<TransactionOptions>` (or `impl Into<StartRecipeOptions>`), which
/// is not possible on the object safe `RecipeServiceFacade`.
#[derive(Clone)]
pub struct RecipeService(Arc<dyn RecipeServiceFacade + Send + Sync>);

impl RecipeService {
    pub fn new(inner: Arc<dyn RecipeServiceFacade + Send + Sync>) -> Self {
        Self(inner)
    }

    pub async fn add_new_default_recipe_with(
        &self,
        options: impl Into<TransactionOptions>,
    ) -> Result<(RecipeId, Recipe), TransactionError> {
        self.0.add_new_default_recipe(options.into()).await
    }

    pub async fn update_recipe_metadata_with(
        &self,
        id: RecipeId,
        data: RecipeMetadata,
        options: impl Into<TransactionOptions>,
    ) -> Result<(), TransactionError> {
        self.0
            .update_recipe_metadata(id, data, options.into())
            .await
    }

    pub async fn delete_recipe_with(
        &self,
        recipe_id: RecipeId,
        options: impl Into<TransactionOptions>,
    ) -> Result<(), TransactionError> {
        self.0.delete_recipe(recipe_id, options.into()).await
    }

    pub async fn delete_recipe(&self, recipe_id: RecipeId) -> Result<(), TransactionError> {
        self.delete_recipe_with(recipe_id, TransactionOptions::default())
            .await
    }

    pub async fn duplicate_recipe_with(
        &self,
        recipe_id: RecipeId,
        options: impl Into<TransactionOptions>,
    ) -> Result<(RecipeId, Recipe), TransactionError> {
        self.0.duplicate_recipe(recipe_id, options.into()).await
    }

    pub async fn duplicate_recipe(
        &self,
        recipe_id: RecipeId,
    ) -> Result<(RecipeId, Recipe), TransactionError> {
        self.duplicate_recipe_with(recipe_id, TransactionOptions::default())
            .await
    }

    pub async fn state(&self) -> ActiveState {
        self.0.state().await
    }

    /// See `RecipeServiceFacade::start_recipe`. A `PendingChangesStrategy` or `TransactionOptions` alone
    /// are accepted as options (`TransactionOptions` alone mean `PendingChangesStrategy::Deny`).
    pub async fn start_recipe_with(
        &self,
        id: RecipeId,
        options: impl Into<StartRecipeOptions>,
    ) -> Result<(), TransactionError> {
        self.0.start_recipe(id, options.into()).await
    }

    /// Like `start_recipe_with` with `PendingChangesStrategy::Deny`
    pub async fn start_recipe(&self, id: RecipeId) -> Result<(), TransactionError> {
        self.start_recipe_with(id, StartRecipeOptions::default())
            .await
    }

    #[deprecated(note = "Renamed to `start_recipe_with`")]
    pub async fn activate_recipe_with(
        &self,
        id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        self.start_recipe_with(id, options).await
    }

    #[deprecated(note = "Renamed to `start_recipe`")]
    pub async fn activate_recipe(&self, id: RecipeId) -> Result<(), TransactionError> {
        self.start_recipe(id).await
    }

    #[deprecated(
        note = "Never worked. Use `start_recipe_with` with `PendingChangesStrategy::Discard`"
    )]
    pub async fn restore_active_with(
        &self,
        _transaction_key: Uuid,
    ) -> Result<(), TransactionError> {
        Err(TransactionError::other(anyhow::anyhow!(
            "Not supported: Use start_recipe_with with PendingChangesStrategy::Discard"
        )))
    }

    #[deprecated(
        note = "Never worked. Use `start_recipe_with` with `PendingChangesStrategy::Discard`"
    )]
    #[allow(deprecated)]
    pub async fn restore_active(&self) -> Result<(), TransactionError> {
        self.restore_active_with(Uuid::new_v4()).await
    }

    pub async fn update_device_params_with(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        values: ParameterUpdate,
        options: impl Into<TransactionOptions>,
    ) -> Result<(), TransactionError> {
        self.0
            .update_device_params(recipe_id, device_id, values, options.into())
            .await
    }

    pub async fn update_device_params(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        values: ParameterUpdate,
    ) -> Result<(), TransactionError> {
        self.update_device_params_with(recipe_id, device_id, values, TransactionOptions::default())
            .await
    }

    pub async fn commit_active_with(&self, transaction_key: Uuid) -> Result<(), TransactionError> {
        self.0.commit_active(transaction_key).await
    }

    pub async fn commit_active(&self) -> Result<(), TransactionError> {
        self.commit_active_with(Uuid::new_v4()).await
    }

    pub async fn delete_device_with(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        options: impl Into<TransactionOptions>,
    ) -> Result<(), TransactionError> {
        self.0
            .delete_device(recipe_id, device_id, options.into())
            .await
    }

    pub async fn delete_device(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
    ) -> Result<(), TransactionError> {
        self.delete_device_with(recipe_id, device_id, TransactionOptions::default())
            .await
    }

    pub async fn restore_committed(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        transaction: Uuid,
    ) -> Result<(), TransactionError> {
        self.0
            .restore_committed(recipe_id, device_id, transaction)
            .await
    }

    pub async fn update_device_name_with(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        name: Name,
        options: impl Into<TransactionOptions>,
    ) -> Result<(), TransactionError> {
        self.0
            .update_device_name(recipe_id, device_id, name, options.into())
            .await
    }

    pub fn get_update_receiver(&self) -> BoxStream<'static, Uuid> {
        self.0.get_update_receiver()
    }
}

/// How to change the active recipe: What happens with uncommitted changes and the transaction
/// the change belongs to. `TransactionOptions` alone (`From`) deny uncommitted changes.
#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct StartRecipeOptions {
    pub strategy: PendingChangesStrategy,
    pub transaction: TransactionOptions,
}

impl StartRecipeOptions {
    pub fn new(strategy: PendingChangesStrategy, transaction: TransactionOptions) -> Self {
        Self {
            strategy,
            transaction,
        }
    }
}

impl From<TransactionOptions> for StartRecipeOptions {
    fn from(transaction: TransactionOptions) -> Self {
        Self {
            strategy: PendingChangesStrategy::default(),
            transaction,
        }
    }
}

impl From<PendingChangesStrategy> for StartRecipeOptions {
    fn from(strategy: PendingChangesStrategy) -> Self {
        Self {
            strategy,
            transaction: TransactionOptions::default(),
        }
    }
}

#[deprecated(note = "Renamed to `RecipeServiceFacade`")]
pub trait RecipeServiceTrait: RecipeServiceFacade {}
#[allow(deprecated)]
impl<T: RecipeServiceFacade + ?Sized> RecipeServiceTrait for T {}

/// The object safe core of the recipe service. Use `RecipeService`, which adds convenience methods
/// (`*_with` accepting `impl Into<...Options>`, and variants with default options).
#[async_trait]
pub trait RecipeServiceFacade {
    async fn add_new_default_recipe(
        &self,
        options: TransactionOptions,
    ) -> Result<(RecipeId, Recipe), TransactionError>;
    async fn update_recipe_metadata(
        &self,
        id: RecipeId,
        data: RecipeMetadata,
        options: TransactionOptions,
    ) -> Result<(), TransactionError>;
    async fn delete_recipe(
        &self,
        recipe_id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(), TransactionError>;
    async fn duplicate_recipe(
        &self,
        recipe_id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(RecipeId, Recipe), TransactionError>;

    async fn state(&self) -> ActiveState;

    /// Changes the active recipe and resolves uncommitted changes of the currently active recipe according to
    /// `options.strategy`.
    ///
    /// If the service controls running devices, it stops all of them first (cooperatively, it waits until they
    /// finished), because device files must not be compared or replaced while a device is using them.
    /// Whether files changed can only be known while the devices are stopped, so the devices are started afterwards
    /// in any case: Also if the recipe is active already, and also if the change is denied because of uncommitted
    /// changes (the devices then continue with the unchanged active recipe).
    /// The only exception is an unknown `id`: It fails without touching the devices.
    ///
    /// Replaces `RecipeRunner::select_recipe`, which behaves like `Deny`.
    ///
    /// Implementations which don't support resolving pending changes only support `PendingChangesStrategy::Deny`
    async fn start_recipe(
        &self,
        id: RecipeId,
        options: StartRecipeOptions,
    ) -> Result<(), TransactionError>;

    async fn update_device_params(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        values: ParameterUpdate,
        options: TransactionOptions,
    ) -> Result<(), TransactionError>;

    async fn commit_active(&self, transaction_key: Uuid) -> Result<(), TransactionError>;

    async fn delete_device(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        options: TransactionOptions,
    ) -> Result<(), TransactionError>;

    // Before having uncommitted recipes, devices were able to be uncommitted
    // This feature became partially obsolete with uncommitted recipes feature.
    async fn restore_committed(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        transaction: Uuid,
    ) -> Result<(), TransactionError>;
    async fn update_device_name(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        name: Name,
        options: TransactionOptions,
    ) -> Result<(), TransactionError>;
    fn get_update_receiver(&self) -> BoxStream<'static, Uuid>;
}

#[derive(Debug, Deserialize, Clone)]
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
