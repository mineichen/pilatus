use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::io::{self, ErrorKind};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use futures::stream::BoxStream;
use futures::{StreamExt, TryStreamExt};
use minfac::{AllRegistered, Registered, ServiceCollection};
use pilatus::device::{ActiveState, ActorErrorUnknownDevice, DeviceContext};
use pilatus::UncommittedChangesError;
use pilatus::{
    clone_directory_deep, device::DeviceId, visit_directory_files, DeviceConfig, GenericConfig,
    InitRecipeListener, Name, ParameterUpdate, Recipe, RecipeExporter, RecipeId, RecipeImporter,
    RecipeMetadata, Recipes, TransactionError, TransactionOptions,
    UntypedDeviceParamsWithVariables, VariableError, Variables, VariablesPatch,
};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::{
    fs,
    io::AsyncRead,
    sync::{broadcast, Mutex},
};
use tracing::{debug, error, trace};
use uuid::Uuid;

use self::actions::DeviceActions;
use self::recipes::RecipesExt;

mod actions;
mod export;
mod fassade;
mod file;
mod import;
mod parameters;
mod recipes;
mod service_builder;

pub use actions::*;
pub use export::*;
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
    c.with::<Registered<Arc<RecipeServiceFassade>>>()
        .register(|x| x as RecipeExporter);

    c.with::<Registered<Arc<RecipeServiceImpl>>>()
        .register(|x| Box::new(RecipeImporterImpl(x)) as RecipeImporter);

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

pub struct RecipeServiceImpl {
    path: PathBuf,
    recipes: Arc<Mutex<Recipes>>,
    device_actions: Arc<dyn DeviceActions>,
    listeners: Vec<InitRecipeListener>,
    update_sender: broadcast::Sender<Uuid>,
    // Can be used to update a Device with change_device_params_on_active_recipe
    // DeviceType -> fn(serde_json::Value, T) -> Result<serde_json::Value, TransactionError>>
    change_strategies: HashMap<(&'static str, TypeId), Box<dyn Any + Send + Sync>>,
}

impl Debug for RecipeServiceImpl {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecipeService")
            .field("path", &self.path)
            .field("recipes", &self.recipes)
            .field("recipe_permissioner", &self.device_actions)
            .finish()
    }
}

impl RecipeServiceImpl {
    async fn add_new_default_recipe(
        &self,
        options: TransactionOptions,
    ) -> Result<(RecipeId, Recipe), TransactionError> {
        let mut recipes = self.recipes.lock().await;
        let mut recipe = Recipe::default();

        // add all default devices
        for listener in self.listeners.iter() {
            listener.call(&mut recipe);
        }

        let new_id = recipes.add_new(recipe.clone());
        self.save_config(&recipes, options.key).await?;

        Ok((new_id, recipe))
    }

    async fn update_recipe_metadata(
        &self,
        id: RecipeId,
        data: RecipeMetadata,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        let raw = data.into_inner();
        let mut recipes = self.recipes.lock().await;

        if id != raw.new_id {
            recipes.update_recipe_id(&id, raw.new_id.clone())?;
        }

        let r = recipes.get_with_id_or_error_mut(&raw.new_id)?;
        r.tags = raw.tags;
        self.save_config(&recipes, options.key)
            .await
            .map_err(Into::into)
    }

    async fn delete_recipe(
        &self,
        recipe_id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        let mut recipes = self.recipes.lock().await;

        let removed = recipes.remove(&recipe_id)?;
        for device_id in removed.devices.keys() {
            // Ok, as RecipeService creates the subfolder (by default "recipe") and therefore remove_dir_all shouldn't accidentally remove too much
            if let Err(e) = tokio::fs::remove_dir_all(&self.get_device_dir(device_id)).await {
                if e.kind() != ErrorKind::NotFound {
                    return Err(e.into());
                }
            }
        }

        self.save_config(&recipes, options.key)
            .await
            .map_err(Into::into)
    }

    async fn duplicate_recipe(
        &self,
        recipe_id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(RecipeId, Recipe), TransactionError> {
        let mut recipes = self.recipes.lock().await;

        let original = recipes.get_with_id_or_error(&recipe_id)?;
        let (new_recipe_id, duplicate) = recipes.build_duplicate(recipe_id, original);

        for (old_id, new_id) in duplicate.mappings.iter() {
            let path = self.get_recipe_dir_path();
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
        recipes.add_inexistent(new_recipe_id.clone(), duplicate.recipe.clone());

        self.save_config(&recipes, options.key).await?;
        Ok((new_recipe_id, duplicate.recipe))
    }
    async fn state(&self) -> ActiveState {
        let recipes = self.recipes.lock().await;
        let has_uncommitted_changes =
            self.check_active_files(&recipes).await.is_err() || recipes.has_active_changes();
        ActiveState::new(recipes.clone(), has_uncommitted_changes)
    }

    pub(super) async fn activate_recipe_with(
        &self,
        id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        let mut recipes = self.recipes.lock().await;

        self.check_active_files(&recipes).await?;

        let active_devices = recipes.set_active(&id)?;
        self.copy_backup_files(active_devices).await?;

        self.save_config(&recipes, options.key)
            .await
            .map_err(Into::into)
    }

    async fn update_device_params(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        values: ParameterUpdate,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        let mut recipes = self.recipes.lock().await;
        let variables = self
            .apply_params(device_id, &values.parameters, values.variables, &recipes)
            .await?;
        let recipe = recipes
            .get_with_id_or_error_mut(&recipe_id)
            .expect("Always works after apply_params");

        options.update_device_params(recipe, device_id, values.parameters)?;
        *recipes.as_mut() = variables;
        self.save_config(&recipes, options.key).await?;
        Ok(())
    }

    async fn restore_active(&self) -> Result<(), TransactionError> {
        Err(TransactionError::Other(anyhow!("Not yet implemented")))
    }

    async fn commit_active(&self, transaction_key: Uuid) -> Result<(), TransactionError> {
        let mut recipes = self.recipes.lock().await;

        self.copy_backup_files(
            recipes
                .active()
                .1
                .devices
                .iter_unordered()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
        )
        .await?;
        recipes.commit_active();
        self.save_config(&recipes, transaction_key).await?;
        Ok(())
    }

    async fn delete_device(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
    ) -> Result<(), TransactionError> {
        let mut recipes = self.recipes.lock().await;
        let recipe = recipes.get_with_id_or_error_mut(&recipe_id)?;
        if recipe.devices.remove(&device_id).is_none() {
            Err(ActorErrorUnknownDevice {
                device_id,
                detail: "Not found in recipe".into(),
            }
            .into())
        } else {
            tokio::fs::remove_dir_all(self.get_device_dir(&device_id))
                .await
                .ok();
            Ok(())
        }
    }

    async fn restore_committed(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        transaction: Uuid,
    ) -> Result<(), TransactionError> {
        let mut recipes = self.recipes.lock().await;
        let restored = recipes
            .get_with_id_or_error_mut(&recipe_id)?
            .get_device_by_id(device_id)?
            .restore_committed()?
            // Even if we get an immutable ref in restore_committed(), recipes is still borrowed mut (Current compiler 'bug')
            .clone();
        let variables = self
            .apply_params(device_id, &restored, Default::default(), &recipes)
            .await?;
        *recipes.as_mut() = variables;
        self.save_config(&recipes, transaction).await?;

        Ok(())
    }

    async fn update_device_name(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        name: Name,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        let mut recipes = self.recipes.lock().await;

        recipes
            .get_with_id_or_error_mut(&recipe_id)?
            .get_device_by_id(device_id)?
            .device_name = name;

        self.save_config(&recipes, options.key).await?;
        Ok(())
    }
    fn get_update_receiver(&self) -> BoxStream<'static, Uuid> {
        tokio_stream::wrappers::BroadcastStream::new(self.update_sender.subscribe())
            .filter_map(|x| async { x.ok() })
            .boxed()
    }
}

impl RecipeServiceImpl {
    // Checks running device-ids only. If Backup contains more devices, differences are detected in Recipes::has_active_changes
    pub async fn check_active_files(&self, recipes: &Recipes) -> Result<(), TransactionError> {
        let backup_root = self.get_recipe_dir_path().join("backup");
        for group in recipes.iter_running_join_backup() {
            let group = group?;
            let running_fs = self.build_device_file_service(group.id);
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
            let mut r_sorted = running_fs.list_recursive().await?;
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
                    || !is_content_equal(File::open(&a).await?, File::open(&b).await?).await?
                {
                    Err(UncommittedChangesError)?;
                }
            }
        }
        Ok(())
    }

    async fn copy_backup_files(
        &self,
        device_ids: impl IntoIterator<Item = DeviceId>,
    ) -> Result<(), TransactionError> {
        let path = self.get_recipe_dir_path();
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

    pub async fn get_owned_devices_from_active(
        &self,
    ) -> (RecipeId, Vec<(DeviceId, DeviceConfig)>, Variables) {
        let mut recipes = self.recipes.lock().await;
        let (id, recipe) = recipes.get_active();
        (
            id,
            recipe
                .devices
                .iter_unordered()
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
            recipes.as_ref().clone(),
        )
    }

    async fn apply_params(
        &self,
        device_id: DeviceId,
        params: &UntypedDeviceParamsWithVariables,
        variables: VariablesPatch,
        recipes: &Recipes,
    ) -> Result<Variables, TransactionError> {
        let usages = if !variables.is_empty() {
            recipes
                .find_variable_usage_in_all_recipes(&variables)
                .collect()
        } else {
            Vec::new()
        };
        let patched_vars = recipes.as_ref().patch(variables);

        let mut has_var_changes_on_active = false;
        let (active_id, _) = recipes.active();

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

        if has_var_changes_on_active || recipes.has_device_on_running(device_id) {
            let edit_device_type = &recipes.get_device_or_error(device_id)?.device_type;
            self.device_actions
                .try_apply(
                    edit_device_type,
                    DeviceContext::new(device_id, patched_vars.clone(), params.clone()),
                )
                .await?;
        }
        Ok(patched_vars)
    }

    #[cfg(any(test, feature = "unstable"))]
    pub async fn clone_device_config(
        &self,
        _recipe_id: RecipeId,
        device_id: DeviceId,
    ) -> Result<DeviceConfig, TransactionError> {
        let recipes = self.recipes.lock().await;
        Ok(recipes.get_device_or_error(device_id)?.clone())
    }

    async fn save_config(&self, recipes: &Recipes, transaction_key: Uuid) -> Result<(), io::Error> {
        let p = self.get_recipe_file_path();
        trace!(path = ?p, "storing json (async)");
        let mut file = tokio::fs::File::create(p).await?;
        file.write_all(&serde_json::to_vec_pretty(recipes)?).await?;
        file.flush().await?;

        if self.update_sender.send(transaction_key).is_err() {
            debug!("Nobody is listening for recipe update");
        }
        Ok(())
    }

    fn get_recipe_file_path(&self) -> PathBuf {
        self.path.clone().join(RECIPES_FILE_NAME)
    }

    fn get_device_dir(&self, device_id: &DeviceId) -> PathBuf {
        self.get_recipe_dir_path().join(device_id.to_string())
    }

    pub fn get_recipe_dir_path(&self) -> &PathBuf {
        &self.path
    }
}

#[cfg(any(test, feature = "unstable"))]
pub(crate) mod unstable {
    use super::{recipes::recipes_try_add_new_with_id, *};
    use pilatus::{device::DeviceId, Recipe, RecipeId, TransactionError, TransactionOptions};
    use std::sync::Arc;
    impl RecipeServiceImpl {
        pub async fn add_recipe_with_id(
            &self,
            id: RecipeId,
            recipe: Recipe,
            options: TransactionOptions,
        ) -> Result<(), TransactionError> {
            let mut recipes = self.recipes.lock().await;
            recipes_try_add_new_with_id(
                &mut recipes,
                id.clone(),
                recipe,
                self.device_actions.as_ref(),
            )
            .await
            .map_err(|_| TransactionError::Other(anyhow::anyhow!("Recipe {id} already exists ")))?;
            self.save_config(&recipes, options.key).await?;
            Ok(())
        }
        pub async fn get_active_id(&self) -> RecipeId {
            let mut recipes = self.recipes.lock().await;
            recipes.get_active().0
        }

        pub async fn get_active(&self) -> (RecipeId, Recipe) {
            let mut recipes = self.recipes.lock().await;
            let (id, r) = recipes.get_active();
            (id, r.clone())
        }

        /// Has no duplication Detection for Device-IDs yet
        /// Implemented for Testing purpose
        pub async fn add_recipe(
            &self,
            r: Recipe,
            options: TransactionOptions,
        ) -> Result<RecipeId, TransactionError> {
            let mut recipes = self.recipes.lock().await;
            let id = recipes.add_new(r);
            self.save_config(&recipes, options.key).await?;
            Ok(id)
        }

        pub async fn add_device_to_recipe(
            &self,
            recipe_id: RecipeId,
            device: DeviceConfig,
            options: TransactionOptions,
        ) -> Result<DeviceId, TransactionError> {
            let mut recipes = self.recipes.lock().await;
            let id = recipes
                .get_with_id_or_error_mut(&recipe_id)?
                .add_device(device);
            self.save_config(&recipes, options.key).await?;
            Ok(id)
        }

        pub async fn add_device_with_id(
            &self,
            recipe_id: RecipeId,
            id: DeviceId,
            device: DeviceConfig,
        ) -> Result<(), TransactionError> {
            let mut recipes = self.recipes.lock().await;
            let recipe = recipes.get_with_id_or_error_mut(&recipe_id)?;
            recipe
                .add_device_with_id(id, device)
                .map_err(|x| TransactionError::Other(x.into()))?;
            self.save_config(&recipes, Uuid::new_v4()).await?;
            Ok(())
        }

        pub async fn add_device_to_active_recipe(
            &self,
            device: DeviceConfig,
            options: TransactionOptions,
        ) -> Result<DeviceId, TransactionError> {
            let mut recipes = self.recipes.lock().await;
            let id = recipes.get_active().1.add_device(device);
            self.save_config(&recipes, options.key).await?;
            Ok(id)
        }

        pub fn create_importer(this: impl Into<Arc<Self>>) -> RecipeImporter {
            let x: Arc<Self> = this.into();
            Box::new(RecipeImporterImpl(x))
        }
    }
}
#[cfg(any(test, feature = "unstable"))]
pub use unstable::*;

async fn is_content_equal(a: impl AsyncRead, b: impl AsyncRead) -> std::io::Result<bool> {
    let mut a = std::pin::pin!(a);
    let mut b = std::pin::pin!(b);
    let mut remaining_a = 0;
    let mut remaining_b = 0;
    let mut buf_a = [0; 4096];
    let mut buf_b = [0; 4096];

    loop {
        let (a_bytes, b_bytes) = futures::future::join(
            a.read(&mut buf_a[remaining_a..]),
            b.read(&mut buf_b[remaining_b..]),
        )
        .await;
        let a_bytes = a_bytes? + remaining_a;
        let b_bytes = b_bytes? + remaining_b;
        let min = a_bytes.min(b_bytes);
        if min == 0 {
            return Ok(a_bytes == b_bytes);
        } else if buf_a[..min] != buf_b[..min] {
            return Ok(false);
        } else {
            remaining_a = a_bytes - min;
            remaining_b = b_bytes - min;
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
    async fn with_multiple_lengths() {
        let a = [0; 4098];
        let b = [0; 4097];
        assert!(
            !is_content_equal(std::io::Cursor::new(a), std::io::Cursor::new(b))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn with_multiple_pages() {
        let a = [0; 4098];
        let mut b = [0; 4098];
        *b.last_mut().unwrap() = 1;
        assert!(
            !is_content_equal(std::io::Cursor::new(a), std::io::Cursor::new(b))
                .await
                .unwrap()
        );
    }
    #[tokio::test]
    async fn with_multiple_same_pages() {
        let a = [0; 4098];
        let b = [0; 4098];
        assert!(
            is_content_equal(std::io::Cursor::new(a), std::io::Cursor::new(b))
                .await
                .unwrap()
        );
    }

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
            }
        )
        .await?;

        #[derive(Deserialize, Debug, PartialEq, Eq)]
        struct Foo {
            test: i32,
        }
        assert_eq!(
            rs.recipe_service
                .recipes
                .lock()
                .await
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
                }
            )
            .await;

        let Err(TransactionError::InvalidVariable(VariableError {recipe_id, reason})) = second_update else {
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

        assert_eq!(dir.path().join("recipes"), rs.recipe_service.path);
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
        let mut file_service = rs.build_device_file_service(device_id);
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
        let mut fs = rs.build_device_file_service(did);
        fs.add_file_unchecked(&RelativeFilePath::new("test.txt").unwrap(), b"test")
            .await
            .unwrap();
        assert_eq!(1, fs.list_recursive().await?.len());
        rs.restore_active().await?;
        assert_eq!(0, fs.list_recursive().await?.len());

        Ok(())
    }

    #[tokio::test]
    async fn set_active_forbidden_with_new_device() -> anyhow::Result<()> {
        let (_dir, rsb) = RecipeServiceFassade::create_temp_builder();
        let rs = rsb.build();
        let r2_id = rs.add_recipe(Recipe::default()).await?;
        rs.add_device_to_active_recipe(DeviceConfig::mock("params"))
            .await?;

        let Err(TransactionError::Other(_)) = rs
            .activate_recipe(r2_id.clone())
            .await else {
                panic!("Expected Other error")
            };

        rs.commit_active(Default::default()).await?;
        rs.activate_recipe(r2_id)
            .await
            .unwrap();
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
        let mut fs = rs.build_device_file_service(r2_d1);
        fs.add_file_unchecked(&"test.txt".try_into()?, b"test")
            .await?;

        match rs
            .activate_recipe(r1_id.clone())
            .await
        {
            Err(TransactionError::Other(_)) => {}
            e => panic!("Unexpected: {e:?}"),
        }
        rs.commit_active(Default::default()).await?;
        rs.activate_recipe(r1_id)
            .await
            .unwrap();

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
        let mut fs = rs.build_device_file_service(r2_d1);
        let initial_filename = "test.txt".try_into()?;
        fs.add_file_unchecked(&initial_filename, content).await?;
        rs.activate_recipe(r2_id).await?;
        fs.remove_file(&initial_filename).await?;
        fs.add_file_unchecked(&"test2.txt".try_into()?, content)
            .await?;

        match rs
            .activate_recipe(r1_id.clone())
            .await
        {
            Err(TransactionError::Other(_)) => {}
            e => panic!("Unexpected: {e:?}"),
        }
        rs.commit_active(Default::default()).await?;
        rs.activate_recipe(r1_id)
            .await
            .unwrap();

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
        
        let rs = RecipeServiceBuilder::new(dir.path(), Arc::new(parameters::LambdaRecipePermissioner::always_ok()))
        .build();
        let dev = rs.clone_device_config(recipe_id.clone(), device_in_other_recipe_id).await?;
        assert_eq!(dev.device_type, "testdevice2".to_string());
        let testdevice2_params = serde_json::from_value::<SampleParams>(serde_json::Value::clone(&dev.params))?;
        assert_eq!(testdevice2_params.foo, 14);
        assert_eq!(testdevice2_params.bar, "Hi".to_string());

        let dev = rs.clone_device_config(recipe_id, device_in_active_recipe_id).await?;
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
        
        let new_device_path_with_file = 'outer: loop {
            for device_id in new_device_config.devices.keys() {
                let device_path = rs.recipe_service.get_device_dir(device_id);
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

       
        let rs = RecipeServiceBuilder::new(dir.path(), Arc::new(parameters::LambdaRecipePermissioner::always_ok())).build();
        let s = rs.clone_device_config(recipe_id, device_id).await.unwrap();
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

       let rs = RecipeServiceBuilder::new(dir.path(), Arc::new(parameters::LambdaRecipePermissioner::always_ok())).build();
       let s = rs.clone_device_config(recipe_id, dev_uuid).await.unwrap();

       assert_eq!(234, serde_json::from_value::<SampleParams>(serde_json::Value::clone(&s.params))?.foo);
       dir.close()?;
        Ok(())
    }
}
