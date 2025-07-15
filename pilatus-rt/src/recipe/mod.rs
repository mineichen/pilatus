use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::io::{self, ErrorKind};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::anyhow;
use futures::stream::BoxStream;
use futures::{StreamExt, TryStreamExt};
use minfac::{AllRegistered, Registered, ServiceCollection};
use pilatus::device::{ActiveState, DeviceContext};
use pilatus::{
    clone_directory_deep, device::DeviceId, visit_directory_files, DeviceConfig, GenericConfig,
    InitRecipeListener, Name, ParameterUpdate, Recipe, RecipeId, RecipeMetadata, Recipes,
    TransactionError, TransactionOptions, UntypedDeviceParamsWithVariables, VariableError,
    Variables, VariablesPatch,
};
use pilatus::{RelativeDirectoryPath, UncommittedChangesError, UnknownDeviceError};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{RwLockReadGuard, RwLockWriteGuard};
use tokio::{
    fs,
    io::AsyncRead,
    sync::{broadcast, RwLock},
};
use tracing::{debug, error, trace};
use uuid::Uuid;

use self::recipes::RecipesExt;

mod actions;
mod export;
mod fassade;
mod file;
mod has_same_content;
mod import;
mod parameters;
mod recipes;
mod service_builder;

pub use actions::*;
pub use fassade::*;
pub use file::TokioFileService;
pub use import::*;
pub use parameters::*;
pub use service_builder::RecipeServiceBuilder;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<(
        Registered<GenericConfig>,
        AllRegistered<InitRecipeListener>,
        Registered<Arc<dyn DeviceActions>>,
        AllRegistered<parameters::ChangeParamsStrategy>,
    )>()
    .register_shared(
        |(conf, initializers, device_actions, change_params_strategies)| {
            let mut builder = RecipeServiceBuilder::new(conf.root, device_actions);
            builder = initializers.fold(builder, |acc, x| acc.with_initializer(x));
            builder = change_params_strategies.fold(builder, |acc, x| acc.with_change_strategy(x));

            Arc::new(builder.build())
        },
    );

    fassade::register_services(c);
    parameters::register_services(c);
    file::register_services(c);
}

#[derive(thiserror::Error, Debug)]
pub enum ChangeDeviceParamsTransactionError {
    #[error("{0:?}")]
    Transaction(TransactionError),
    #[error("Unknown combination for DeviceType and MessageType")]
    UnknownModifier,
}

impl<X: Into<TransactionError>> From<X> for ChangeDeviceParamsTransactionError {
    fn from(x: X) -> Self {
        ChangeDeviceParamsTransactionError::Transaction(x.into())
    }
}

const RECIPES_FILE_NAME: &str = "recipes.json";

pub struct RecipeServiceAccessor {
    path: PathBuf,
    recipes: Arc<RwLock<Recipes>>,
    device_actions: Arc<dyn DeviceActions>,
    listeners: Vec<InitRecipeListener>,
    update_sender: broadcast::Sender<Uuid>,
    // Can be used to update a Device with change_device_params_on_active_recipe
    // DeviceType -> fn(serde_json::Value, T) -> Result<serde_json::Value, TransactionError>>
    change_strategies: HashMap<(&'static str, TypeId), Box<dyn Any + Send + Sync>>,
}

pub struct RecipeDataService<'a, T: 'a> {
    path: &'a Path,
    recipes: T,
    device_actions: &'a dyn DeviceActions,
    listeners: &'a [InitRecipeListener],
    update_sender: &'a broadcast::Sender<Uuid>,
    change_strategies: &'a HashMap<(&'static str, TypeId), Box<dyn Any + Send + Sync>>,
}

impl<T: Deref<Target = Recipes>> RecipeDataService<'_, T> {
    async fn state(&self) -> ActiveState {
        let has_uncommitted_changes =
            self.check_active_files().await.is_err() || self.recipes.has_active_changes();
        ActiveState::new(Recipes::clone(&self.recipes), has_uncommitted_changes)
    }

    // Checks running device-ids only. If Backup contains more devices, differences are detected in Recipes::has_active_changes
    pub async fn check_active_files(&self) -> Result<(), TransactionError> {
        let backup_root = self.recipe_dir_path().join("backup");
        for group in self.recipes.iter_running_join_backup() {
            let group = group?;
            let running_fs = TokioFileService::builder(self.recipe_dir_path()).build(group.id);
            let backup_device_dir = backup_root.join(group.id.to_string());
            let mut b_sorted: Vec<_> = pilatus::visit_directory_files(&backup_device_dir)
                .take_while(|f| {
                    std::future::ready(if let Err(e) = f {
                        e.kind() != std::io::ErrorKind::NotFound
                    } else {
                        true
                    })
                })
                .map(|f| f.map(|f| f.path()))
                .try_collect()
                .await?;
            let mut r_sorted = running_fs
                .list_recursive(RelativeDirectoryPath::root())
                .await?;
            if b_sorted.len() != r_sorted.len() {
                Err(UncommittedChangesError)?;
            }

            b_sorted.sort();
            r_sorted.sort();
            for (a, b) in b_sorted.into_iter().zip(r_sorted) {
                let relative_a = a.strip_prefix(&backup_device_dir).unwrap_or_else(|e| {
                    panic!(
                        "Was constructed with backup_root above {:?}, {:?} ({e:?})",
                        a, &backup_device_dir,
                    )
                });
                let relative_b = b.strip_prefix(running_fs.get_root()).unwrap_or_else(|e| {
                    panic!(
                        "Was constructed with running_fs above {:?}, {:?} ({e:?})",
                        b,
                        running_fs.get_root(),
                    )
                });

                if relative_a != relative_b
                    || !has_same_content::has_same_content(
                        File::open(&a).await?,
                        File::open(&b).await?,
                    )
                    .await?
                {
                    Err(UncommittedChangesError)?;
                }
            }
        }
        Ok(())
    }

    pub async fn get_owned_devices_from_active(
        &self,
    ) -> (RecipeId, Vec<(DeviceId, DeviceConfig)>, Variables) {
        let (id, recipe) = self.recipes.active();
        (
            id,
            recipe
                .devices
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
            self.recipes.as_ref().clone(),
        )
    }

    pub fn recipe_dir_path(&self) -> &Path {
        self.path
    }

    fn get_recipe_file_path(&self) -> PathBuf {
        self.path.join(RECIPES_FILE_NAME)
    }

    fn device_dir(&self, device_id: &DeviceId) -> PathBuf {
        self.path.join(device_id.to_string())
    }
}

impl<T: DerefMut<Target = Recipes>> RecipeDataService<'_, T> {
    async fn delete_device(
        &mut self,
        recipe_id: RecipeId,
        device_id: DeviceId,
    ) -> Result<(), TransactionError> {
        let recipe = self.recipes.get_with_id_or_error_mut(&recipe_id)?;
        if recipe.devices.shift_remove(&device_id).is_none() {
            Err(UnknownDeviceError(device_id))?
        } else {
            tokio::fs::remove_dir_all(self.device_dir(&device_id))
                .await
                .ok();
        };
        Ok(())
    }

    async fn add_new_default_recipe(&mut self) -> Result<(RecipeId, Recipe), TransactionError> {
        let mut recipe = Recipe::default();

        // add all default devices
        for listener in self.listeners.iter() {
            listener.call(&mut recipe);
        }

        let new_id = self.recipes.add_new(recipe.clone());

        Ok((new_id, recipe))
    }

    async fn update_recipe_metadata(
        &mut self,
        id: RecipeId,
        data: RecipeMetadata,
    ) -> Result<(), TransactionError> {
        let raw = data.into_inner();

        if id != raw.new_id {
            self.recipes.update_recipe_id(&id, raw.new_id.clone())?;
        }

        let r = self.recipes.get_with_id_or_error_mut(&raw.new_id)?;
        r.tags = raw.tags;
        Ok(())
    }

    async fn delete_recipe(&mut self, recipe_id: RecipeId) -> Result<(), TransactionError> {
        let removed = self.recipes.remove(&recipe_id)?;
        for device_id in removed.devices.keys() {
            // Ok, as RecipeService creates the subfolder (by default "recipe") and therefore remove_dir_all shouldn't accidentally remove too much
            if let Err(e) = tokio::fs::remove_dir_all(&self.device_dir(device_id)).await {
                if e.kind() != ErrorKind::NotFound {
                    return Err(e.into());
                }
            }
        }
        Ok(())
    }

    async fn commit_active(&mut self) -> Result<(), TransactionError> {
        self.copy_backup_files(
            self.recipes
                .active()
                .1
                .devices
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
        )
        .await?;
        self.recipes.commit_active();
        Ok(())
    }

    async fn update_device_params(
        &mut self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        values: ParameterUpdate,
        options: &TransactionOptions,
    ) -> Result<(), TransactionError> {
        let variables = self
            .apply_params(device_id, &values.parameters, values.variables)
            .await?;
        let recipe = self.recipes.get_with_id_or_error_mut(&recipe_id)?;

        options.update_device_params(recipe, device_id, values.parameters)?;
        *self.recipes.as_mut() = variables;
        Ok(())
    }

    async fn apply_params(
        &self,
        device_id: DeviceId,
        params: &UntypedDeviceParamsWithVariables,
        variables: VariablesPatch,
    ) -> Result<Variables, TransactionError> {
        let usages = if !variables.is_empty() {
            self.recipes
                .find_variable_usage_in_all_recipes(&variables)
                .collect()
        } else {
            Vec::new()
        };
        let patched_vars = self.recipes.as_ref().patch(variables);

        let mut has_var_changes_on_active = false;
        let (active_id, _) = self.recipes.active();

        for (recipe_id, device_type, device_id, params) in usages {
            if recipe_id == active_id {
                has_var_changes_on_active = true;
                continue;
            }

            let update = self
                .device_actions
                .validate(
                    &device_type,
                    DeviceContext::new(device_id, patched_vars.clone(), params.clone()),
                )
                .await
                .map_err(|e| VariableError::from((recipe_id, e)))?;

            if update.into_data_if_no_changes().is_none() {
                error!("Unexpected changes for device after Variable-Update. All devices should be upgraded on startup");
                return Err(TransactionError::Other(anyhow::anyhow!(
                    "Unexpected migration"
                )));
            }
        }

        if has_var_changes_on_active || self.recipes.has_device_on_running(device_id) {
            let edit_device_type = &self.recipes.get_device_or_error(device_id)?.device_type;
            self.device_actions
                .try_apply(
                    edit_device_type,
                    DeviceContext::new(device_id, patched_vars.clone(), params.clone()),
                )
                .await?;
        }
        Ok(patched_vars)
    }

    async fn restore_committed(
        &mut self,
        recipe_id: RecipeId,
        device_id: DeviceId,
    ) -> Result<(), TransactionError> {
        let restored = self
            .recipes
            .get_with_id_or_error_mut(&recipe_id)?
            .device_by_id_mut(device_id)?
            .restore_committed()?
            // Even if we get an immutable ref in restore_committed(), recipes is still borrowed mut (Current compiler 'bug')
            .clone();
        let variables = self
            .apply_params(device_id, &restored, Default::default())
            .await?;
        *self.recipes.as_mut() = variables;

        Ok(())
    }

    async fn restore_active(&mut self) -> Result<(), TransactionError> {
        Err(TransactionError::Other(anyhow!("Not yet implemented")))
    }

    pub(super) async fn activate_recipe(&mut self, id: RecipeId) -> Result<(), TransactionError> {
        self.check_active_files().await?;

        let active_devices = self.recipes.set_active(&id)?;
        self.copy_backup_files(active_devices).await
    }

    async fn copy_backup_files(
        &self,
        device_ids: impl IntoIterator<Item = DeviceId>,
    ) -> Result<(), TransactionError> {
        let path = self.recipe_dir_path();
        let dst_folder = path.join("backup");
        tokio::fs::remove_dir_all(&dst_folder).await.ok();

        for device_id in device_ids {
            let device_id_str = device_id.to_string();
            let src_path = path.join(&device_id_str);
            let dst_path = dst_folder.join(device_id_str);
            if let Ok(meta) = fs::metadata(&src_path).await {
                if meta.is_dir() {
                    clone_directory_deep(&src_path, dst_path)
                        .await
                        .map_err(TransactionError::from_io_producer(&src_path))?;
                }
            }
        }
        Ok(())
    }

    async fn duplicate_recipe(
        &mut self,
        recipe_id: RecipeId,
    ) -> Result<(RecipeId, Recipe), TransactionError> {
        let original = self.recipes.get_with_id_or_error(&recipe_id)?;
        let (new_recipe_id, duplicate) = self.recipes.build_duplicate(recipe_id, original);

        for (old_id, new_id) in duplicate.mappings.iter() {
            let path = self.recipe_dir_path();
            let src_path = path.join(old_id.to_string());
            let dst_path = path.join(new_id.to_string());
            if let Ok(meta) = fs::metadata(&src_path).await {
                if meta.is_dir() {
                    clone_directory_deep(&src_path, dst_path)
                        .await
                        .map_err(TransactionError::from_io_producer(&src_path))?;
                }
            }
        }
        let mut duplicate = duplicate.into_inner();
        duplicate.recipe.created = chrono::Utc::now();
        self.recipes
            .add_inexistent(new_recipe_id.clone(), duplicate.recipe.clone());

        Ok((new_recipe_id, duplicate.recipe))
    }

    async fn update_device_name(
        &mut self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        name: Name,
    ) -> Result<(), TransactionError> {
        self.recipes
            .get_with_id_or_error_mut(&recipe_id)?
            .device_by_id_mut(device_id)?
            .device_name = name;

        Ok(())
    }

    async fn commit(&self, transaction_key: Uuid) -> io::Result<()> {
        let p = self.get_recipe_file_path();
        trace!(path = ?p, "storing json (async)");
        let mut file = tokio::fs::File::create(p).await?;
        let recipes: &Recipes = &self.recipes;
        file.write_all(&serde_json::to_vec_pretty(recipes)?).await?;
        file.flush().await?;

        if self.update_sender.send(transaction_key).is_err() {
            debug!("Nobody is listening for recipe update");
        }
        Ok(())
    }
}

impl Debug for RecipeServiceAccessor {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecipeService")
            .field("path", &self.path)
            .field("recipes", &self.recipes)
            .field("recipe_permissioner", &self.device_actions)
            .finish()
    }
}

impl RecipeServiceAccessor {
    async fn write(&self) -> RecipeDataService<RwLockWriteGuard<'_, Recipes>> {
        RecipeDataService {
            path: &self.path,
            recipes: self.recipes.write().await,
            device_actions: self.device_actions.deref(),
            listeners: &self.listeners,
            update_sender: &self.update_sender,
            change_strategies: &self.change_strategies,
        }
    }
    async fn read(&self) -> RecipeDataService<RwLockReadGuard<'_, Recipes>> {
        RecipeDataService {
            path: &self.path,
            recipes: self.recipes.read().await,
            device_actions: self.device_actions.deref(),
            listeners: &self.listeners,
            update_sender: &self.update_sender,
            change_strategies: &self.change_strategies,
        }
    }

    fn get_update_receiver(&self) -> BoxStream<'static, Uuid> {
        tokio_stream::wrappers::BroadcastStream::new(self.update_sender.subscribe())
            .filter_map(|x| async { x.ok() })
            .boxed()
    }
}

#[cfg(any(test, feature = "unstable"))]
pub(crate) mod unstable {
    use super::{recipes::recipes_try_add_new_with_id, *};
    use pilatus::{device::DeviceId, Recipe, RecipeId, TransactionError};

    impl<T: Deref<Target = Recipes>> RecipeDataService<'_, T> {
        pub fn device_config(
            &self,
            _recipe_id: RecipeId,
            device_id: DeviceId,
        ) -> Result<DeviceConfig, TransactionError> {
            Ok(self.recipes.get_device_or_error(device_id)?.clone())
        }
        pub fn get_active_id(&self) -> RecipeId {
            self.recipes.active().0
        }
    }

    impl<T: DerefMut<Target = Recipes>> RecipeDataService<'_, T> {
        pub async fn add_recipe_with_id(
            &mut self,
            id: RecipeId,
            recipe: Recipe,
        ) -> Result<(), TransactionError> {
            recipes_try_add_new_with_id(&mut self.recipes, id.clone(), recipe, self.device_actions)
                .await
                .map_err(|(_, e)| e)
        }

        /// Has no duplication Detection for Device-IDs yet
        /// Implemented for Testing purpose
        pub async fn add_recipe(&mut self, r: Recipe) -> Result<RecipeId, TransactionError> {
            let id = self.recipes.add_new(r);
            Ok(id)
        }
        pub async fn add_device_to_recipe(
            &mut self,
            recipe_id: RecipeId,
            device: DeviceConfig,
        ) -> Result<DeviceId, TransactionError> {
            let id = self
                .recipes
                .get_with_id_or_error_mut(&recipe_id)?
                .add_device(device);
            Ok(id)
        }

        pub async fn add_device_with_id(
            &mut self,
            recipe_id: RecipeId,
            id: DeviceId,
            device: DeviceConfig,
        ) -> Result<(), TransactionError> {
            let recipe = self.recipes.get_with_id_or_error_mut(&recipe_id)?;
            recipe
                .add_device_with_id(id, device)
                .map_err(|x| TransactionError::Other(x.into()))?;
            Ok(())
        }

        pub async fn add_device_to_active_recipe(
            &mut self,
            device: DeviceConfig,
        ) -> Result<DeviceId, TransactionError> {
            let id = self.recipes.get_active().1.add_device(device);
            Ok(id)
        }
    }
}

#[cfg(test)]
mod tests {

    use serde::Deserialize;
    use serde_json::json;

    use pilatus::{RecipeServiceTrait, RelativeFilePath, UpdateParamsMessageError};

    use super::*;

    #[tokio::test]
    async fn change_to_new_variable() -> anyhow::Result<()> {
        let (dir, rsb) = RecipeServiceFassade::create_temp_builder();
        let rs = rsb
            .replace_permissioner(Arc::new(
                super::parameters::LambdaRecipePermissioner::with_validator(|| {
                    Err(UpdateParamsMessageError::VariableError("TEST".into()).into())
                }),
            ))
            .build();

        let device_id = rs
            .add_device_to_active_recipe(DeviceConfig::new_unchecked(
                "my_type",
                "MyDevice",
                json!({ "test": 1}),
            ))
            .await?;
        let active_id = rs.get_active_id().await;
        let parameters = serde_json::from_value::<UntypedDeviceParamsWithVariables>(
            json!({ "test": {"__var": "var1"}}),
        )?;
        rs.update_device_params(
            active_id.clone(),
            device_id,
            ParameterUpdate {
                parameters: parameters.clone(),
                variables: std::iter::once((
                    "var1".to_string(),
                    serde_json::from_str("42").unwrap(),
                ))
                .collect(),
            },
        )
        .await?;

        #[derive(Deserialize, Debug, PartialEq, Eq)]
        struct Foo {
            test: i32,
        }
        assert_eq!(
            rs.recipe_service_read()
                .await
                .recipes
                .as_ref()
                .resolve(&parameters)?
                .params_as::<Foo>()?,
            Foo { test: 42 }
        );

        let (clone_id, _) = rs.duplicate_recipe(active_id.clone()).await?;
        let second_update = rs
            .update_device_params(
                active_id,
                device_id,
                ParameterUpdate {
                    parameters: parameters.clone(),
                    variables: std::iter::once((
                        "var1".to_string(),
                        serde_json::from_str("4242").unwrap(),
                    ))
                    .collect(),
                },
            )
            .await;

        let Err(TransactionError::InvalidVariable(VariableError { recipe_id, reason })) =
            second_update
        else {
            panic!("Should fail as the clone is asked to be ok, but it returns an Error: {second_update:?}");
        };

        assert_eq!(recipe_id, clone_id);
        assert!(
            reason.to_string().contains("TEST"),
            "Expected '{reason:?}' to contain 'TEST'"
        );

        dir.close()?;
        Ok(())
    }

    #[tokio::test]
    async fn test_path_property_assignment() -> anyhow::Result<()> {
        let (dir, rsb) = RecipeServiceFassade::create_temp_builder();
        let rs = rsb.build();

        assert_eq!(dir.path().join("recipes"), rs.recipe_dir_path());
        dir.close()?;
        Ok(())
    }

    #[tokio::test]
    async fn delete_device() -> anyhow::Result<()> {
        let (_dir, rsb) = RecipeServiceFassade::create_temp_builder();
        let rs = rsb.build();
        let active_id = rs.get_active_id().await;
        let device_id = rs
            .add_device_to_active_recipe(DeviceConfig::mock("params"))
            .await?;
        let device_id_with_file = rs
            .add_device_to_active_recipe(DeviceConfig::mock("params"))
            .await?;
        let file_service = rs.build_device_file_service(device_id);
        file_service
            .add_file_unchecked(&"bar/test.txt".try_into()?, b"content")
            .await?;

        rs.delete_device(active_id.clone(), device_id).await?;
        rs.delete_device(active_id, device_id_with_file).await?;
        assert!(!file_service.get_root().exists());

        Ok(())
    }

    #[tokio::test]
    async fn set_active_without_changes() -> anyhow::Result<()> {
        let (_dir, rsb) = RecipeServiceFassade::create_temp_builder();
        let rs = rsb.build();
        let r1_id = rs.get_active_id().await;
        let mut r2 = Recipe::default();
        let r2_d1 = r2.add_device(DeviceConfig::mock(""));
        r2.add_device(DeviceConfig::mock(
            serde_json::json!({ "id": r2_d1.to_string() }),
        ));

        rs.build_device_file_service(r2_d1)
            .add_file_unchecked(&RelativeFilePath::new("test.txt").unwrap(), b"test")
            .await?;
        let r2_id = rs.add_recipe(r2).await?;
        rs.activate_recipe(r2_id).await?;
        rs.activate_recipe(r1_id).await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "Not yet implemented"]
    async fn discard_active_with_new_device_with_files_removes_folder() -> anyhow::Result<()> {
        let (_dir, rsb) = RecipeServiceFassade::create_temp_builder();
        let rs = rsb.build();
        let did = rs
            .add_device_to_active_recipe(DeviceConfig::mock(""))
            .await?;
        let fs = rs.build_device_file_service(did);
        fs.add_file_unchecked(&RelativeFilePath::new("test.txt").unwrap(), b"test")
            .await
            .unwrap();
        assert_eq!(
            1,
            fs.list_recursive(RelativeDirectoryPath::root())
                .await?
                .len()
        );
        rs.restore_active().await?;
        assert_eq!(
            0,
            fs.list_recursive(RelativeDirectoryPath::root())
                .await?
                .len()
        );

        Ok(())
    }

    #[tokio::test]
    async fn set_active_forbidden_with_new_device() -> anyhow::Result<()> {
        let (_dir, rsb) = RecipeServiceFassade::create_temp_builder();
        let rs = rsb.build();
        let r2_id = rs.add_recipe(Recipe::default()).await?;
        rs.add_device_to_active_recipe(DeviceConfig::mock("params"))
            .await?;

        let Err(TransactionError::Other(_)) = rs.activate_recipe(r2_id.clone()).await else {
            panic!("Expected Other error")
        };

        rs.commit_active().await?;
        rs.activate_recipe(r2_id).await.unwrap();
        Ok(())
    }

    #[tokio::test]
    async fn set_active_with_fs_change() -> anyhow::Result<()> {
        let (_dir, rsb) = RecipeServiceFassade::create_temp_builder();
        let rs = rsb.build();
        let mut r2 = Recipe::default();
        let r2_d1 = r2.add_device(DeviceConfig::mock("params"));
        let r2_id = rs.add_recipe(r2).await?;
        let r1_id = rs.get_active_id().await;

        rs.activate_recipe(r2_id).await?;
        let fs = rs.build_device_file_service(r2_d1);
        fs.add_file_unchecked(&"test.txt".try_into()?, b"test")
            .await?;

        match rs.activate_recipe(r1_id.clone()).await {
            Err(TransactionError::Other(_)) => {}
            e => panic!("Unexpected: {e:?}"),
        }
        rs.commit_active().await?;
        rs.activate_recipe(r1_id).await.unwrap();

        Ok(())
    }

    #[tokio::test]
    async fn set_active_with_renamed_file() -> anyhow::Result<()> {
        let content = b"test";
        let (_dir, rsb) = RecipeServiceFassade::create_temp_builder();
        let rs = rsb.build();
        let mut r2 = Recipe::default();
        let r2_d1 = r2.add_device(DeviceConfig::mock("params"));
        let r2_id = rs.add_recipe(r2).await?;
        let r1_id = rs.get_active_id().await;
        let fs = rs.build_device_file_service(r2_d1);
        let initial_filename = "test.txt".try_into()?;
        fs.add_file_unchecked(&initial_filename, content).await?;
        rs.activate_recipe(r2_id).await?;
        fs.remove_file(&initial_filename).await?;
        fs.add_file_unchecked(&"test2.txt".try_into()?, content)
            .await?;

        match rs.activate_recipe(r1_id.clone()).await {
            Err(TransactionError::Other(_)) => {}
            e => panic!("Unexpected: {e:?}"),
        }
        rs.commit_active().await?;
        rs.activate_recipe(r1_id).await.unwrap();

        Ok(())
    }

    #[tokio::test]
    #[rustfmt::skip]
    async fn test_add_device_and_recipe() -> anyhow::Result<()> {
        let (dir, rsb) = RecipeServiceFassade::create_temp_builder();
        let rs = rsb.build();

        #[derive(Debug,  serde::Serialize, serde::Deserialize, PartialEq, Eq)]
        struct SampleParams {foo: u32, bar: String}

        let device = DeviceConfig::mock(SampleParams {foo: 12, bar: "Hallo".to_string()});
        let device2 = DeviceConfig::new_unchecked(
            "testdevice2","testdevice2name",SampleParams {foo: 14, bar: "Hi".to_string()});
        let recipe = Recipe::default();
        let recipe_id = rs.add_recipe(recipe).await.unwrap();
        let device_in_active_recipe_id = rs.add_device_to_active_recipe(device).await.unwrap();
        let device_in_other_recipe_id = rs.add_device_to_recipe(recipe_id.clone(), device2).await.unwrap();
        drop(rs); //all data should be saved to file at this point

        //try to read data from from file

        let rs = RecipeServiceFassadeBuilder::new(dir.path(), Arc::new(parameters::LambdaRecipePermissioner::always_ok()))
        .build();
        let dev = rs.device_config(recipe_id.clone(), device_in_other_recipe_id).await?;
        assert_eq!(dev.device_type, "testdevice2".to_string());
        let testdevice2_params = serde_json::from_value::<SampleParams>(serde_json::Value::clone(&dev.params))?;
        assert_eq!(testdevice2_params.foo, 14);
        assert_eq!(testdevice2_params.bar, "Hi".to_string());

        let dev = rs.device_config(recipe_id, device_in_active_recipe_id).await?;
        assert_eq!(dev.device_type, "testdevice".to_string());
        let testdevice_params = serde_json::from_value::<SampleParams>(serde_json::Value::clone(&dev.params))?;
        assert_eq!(testdevice_params.foo, 12);
        assert_eq!(testdevice_params.bar, "Hallo".to_string());

        dir.close()?;
        Ok(())
    }

    #[tokio::test]
    #[rustfmt::skip]
    async fn test_clone_and_delete() -> anyhow::Result<()> {
        let (dir, rsb) = RecipeServiceFassade::create_temp_builder();
        let rs = rsb.build();

        #[derive(Debug,  serde::Serialize, serde::Deserialize, PartialEq, Eq)]
        struct SampleParams {foo: u32, reference: DeviceId}

        let device = DeviceConfig::mock(SampleParams {foo: 12, reference: DeviceId::new_v4()});
        let device_in_active_recipe_id = rs.add_device_to_active_recipe(device).await.unwrap();

        let device2 = DeviceConfig::new_unchecked(
            "testdevice2", "testdevice2name", SampleParams {foo: 14, reference: device_in_active_recipe_id});
        let device_in_other_recipe_id = rs.add_device_to_active_recipe(device2).await.unwrap();

        rs.create_device_file(device_in_active_recipe_id, "my_file.txt", b"test").await;

        let (new_recipe_id, new_device_config) = rs.duplicate_recipe(rs.get_active_id().await).await.unwrap();
        assert!(!new_device_config.devices.contains_key(&device_in_other_recipe_id), "Clone contains device with the same id as in the original");

        let new_device_path_with_file = 'outer: {
            for device_id in new_device_config.devices.keys() {
                let device_path = rs.device_dir(device_id);
                if tokio::fs::metadata(&device_path).await.is_ok() {
                    break 'outer device_path;
                }
            }
            panic!("Files were not cloned for new Device");
        };

        rs.delete_recipe(new_recipe_id).await.expect("Can delete freshly cloned");
        assert!(tokio::fs::metadata(&new_device_path_with_file).await.is_err());

        dir.close()?;
        Ok(())
    }

    #[tokio::test]
    #[rustfmt::skip]
    async fn test_update_device() -> anyhow::Result<()> {
        let (dir, rsb) = RecipeServiceFassade::create_temp_builder();
        let rs = rsb.build();
        let recipe_id = rs.get_active_id().await;

        #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
        struct SampleParams {foo: u32, bar: String}

        let initial_params = SampleParams {foo: 12, bar: "Hallo".to_string()};

        let mut device = DeviceConfig::mock(&initial_params);
        let device_id = rs.add_device_to_active_recipe(device.clone()).await.unwrap();

        let new_params = SampleParams { foo: 42, ..initial_params};
        device.params = UntypedDeviceParamsWithVariables::from_serializable(&new_params)?;
        rs.update_device_params(recipe_id.clone(), device_id, ParameterUpdate {
            parameters: device.params.clone(),
            variables: Default::default(),
        }).await.unwrap();
        drop(rs); //all data should be saved to file at this point


        let rs = RecipeServiceFassadeBuilder::new(dir.path(), Arc::new(parameters::LambdaRecipePermissioner::always_ok())).build();
        let s = rs.device_config(recipe_id, device_id).await.unwrap();
        assert_eq!(device.params, s.params);
        dir.close()?;
        Ok(())
    }

    #[tokio::test]
    #[rustfmt::skip]
    async fn test_update_device_params() -> anyhow::Result<()> {
        let (dir, rsb) = RecipeServiceFassade::create_temp_builder();
        let rs = rsb.build();

        let recipe_id = rs.get_active_id().await;

        #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone)]
        struct SampleParams {
            foo: u32,
            bar: String,
        }
        let device = DeviceConfig::mock(SampleParams {foo: 12, bar: "Hallo".to_string()});
        let dev_uuid = rs.add_device_to_active_recipe(device.clone()).await?;

        //update params
        let params = SampleParams{foo: 234, bar: "huhu".to_string()};

        rs.update_device_params(recipe_id.clone(), dev_uuid, ParameterUpdate {
            parameters: UntypedDeviceParamsWithVariables::from_serializable(&params)?,
            variables: Default::default(),
        }).await?;
        drop(rs); //all data should be saved to file at this point

       let rs = RecipeServiceFassadeBuilder::new(dir.path(), Arc::new(parameters::LambdaRecipePermissioner::always_ok())).build();
       let s = rs.device_config(recipe_id, dev_uuid).await.unwrap();

       assert_eq!(234, serde_json::from_value::<SampleParams>(serde_json::Value::clone(&s.params))?.foo);
       dir.close()?;
        Ok(())
    }
}
