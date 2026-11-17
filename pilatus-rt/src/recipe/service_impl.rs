use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use minfac::{Registered, ServiceCollection};
use pilatus::device::ActiveState;
#[allow(deprecated)]
use pilatus::device::RecipeRunnerTrait;
use pilatus::{
    device::DeviceId, Name, ParameterUpdate, PendingChangesStrategy, Recipe, RecipeId,
    RecipeMetadata, RecipeService, RecipeServiceFacade, StartRecipeOptions, TransactionError,
    TransactionOptions, UnknownRecipeError,
};
use pilatus::{FileServiceBuilder, RecipeExporter, RecipeImporter};
use tokio::sync::{RwLockReadGuard, RwLockWriteGuard};
use uuid::Uuid;

use super::{RecipeDataService, RecipeImporterImpl, RecipeServiceAccessor};
use crate::device::RecipeRunnerState;
use crate::TokioFileService;

mod builder;

#[cfg(any(test, feature = "unstable"))]
pub use builder::*;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<(
        Registered<Arc<RecipeServiceAccessor>>,
        Registered<Arc<RecipeRunnerState>>,
    )>()
    .register(|(recipe_service, runner)| {
        Arc::new(RecipeServiceImpl {
            runner: Some(runner),
            recipe_service,
        })
    })
    .alias(|x| RecipeService::new(x));
    c.with::<Registered<Arc<RecipeServiceImpl>>>()
        .register(|x| x as RecipeExporter);

    c.with::<Registered<Arc<RecipeServiceImpl>>>()
        .register(|x| Box::new(RecipeImporterImpl(x)) as RecipeImporter);
}

#[deprecated(note = "Renamed to `RecipeServiceImpl`")]
pub type RecipeServiceFassade = RecipeServiceImpl;

#[derive(Clone, Debug)]
pub struct RecipeServiceImpl {
    recipe_service: Arc<RecipeServiceAccessor>,
    // Missing, if the service was built without a running system (e.g. with `RecipeServiceImplBuilder`): There are no devices to stop
    runner: Option<Arc<RecipeRunnerState>>,
}

#[allow(deprecated)]
#[async_trait]
impl RecipeRunnerTrait for RecipeServiceImpl {
    async fn select_recipe(&self, recipe_id: RecipeId) -> anyhow::Result<()> {
        self.start_recipe_anyhow(recipe_id, PendingChangesStrategy::Deny.into())
            .await
    }
}

impl RecipeServiceImpl {
    /// Errors of the recipe change are `TransactionError`s within anyhow, errors of the runner itself
    /// (e.g. not ready) are plain anyhow errors, which is the error format of the legacy `RecipeRunnerTrait`
    async fn start_recipe_anyhow(
        &self,
        id: RecipeId,
        options: StartRecipeOptions,
    ) -> anyhow::Result<()> {
        let Some(runner) = &self.runner else {
            return Ok(self.start_recipe_while_stopped(id, options).await?);
        };
        // Known without touching files, so the devices keep running. Everything else
        // (e.g. file changes) can only be detected while the devices are stopped
        let is_known = self.recipe_service_read().await.recipes.has_recipe(&id);
        if !is_known {
            return Err(TransactionError::from(UnknownRecipeError(id)).into());
        }
        runner.start_recipe(id, options).await
    }

    /// Files are compared and replaced, so this must only be called while no device is running:
    /// By the runner after it stopped all devices, or if there is no runner.
    pub(crate) async fn start_recipe_while_stopped(
        &self,
        id: RecipeId,
        options: StartRecipeOptions,
    ) -> Result<(), TransactionError> {
        let mut s = self.recipe_service_write().await;
        s.activate_recipe(id, options.strategy).await?;
        s.commit(options.transaction.key).await?;
        Ok(())
    }

    pub(super) fn recipe_service(&self) -> &RecipeServiceAccessor {
        &self.recipe_service
    }
    pub async fn recipe_service_read(
        &self,
    ) -> RecipeDataService<'_, RwLockReadGuard<'_, pilatus::Recipes>> {
        self.recipe_service.read().await
    }

    pub async fn recipe_service_write(
        &self,
    ) -> RecipeDataService<'_, RwLockWriteGuard<'_, pilatus::Recipes>> {
        self.recipe_service.write().await
    }
    pub fn recipe_dir_path(&self) -> &Path {
        &self.recipe_service.path
    }
    pub(super) fn build_file_service(&self) -> FileServiceBuilder {
        TokioFileService::builder(self.recipe_dir_path())
    }
}

#[async_trait]
impl RecipeServiceFacade for RecipeServiceImpl {
    async fn add_new_default_recipe(
        &self,
        options: TransactionOptions,
    ) -> Result<(RecipeId, Recipe), TransactionError> {
        let mut s = self.recipe_service_write().await;
        let r = s.add_new_default_recipe().await?;
        s.commit(options.key).await?;
        Ok(r)
    }

    async fn update_recipe_metadata(
        &self,
        id: RecipeId,
        data: RecipeMetadata,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        let mut s = self.recipe_service_write().await;
        s.update_recipe_metadata(id, data).await?;
        s.commit(options.key).await?;
        Ok(())
    }

    async fn delete_recipe(
        &self,
        recipe_id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        let mut s = self.recipe_service_write().await;
        s.delete_recipe(recipe_id).await?;
        s.commit(options.key).await?;
        Ok(())
    }

    async fn duplicate_recipe(
        &self,
        recipe_id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(RecipeId, Recipe), TransactionError> {
        let mut s = self.recipe_service_write().await;
        let r = s.duplicate_recipe(recipe_id).await?;
        s.commit(options.key).await?;
        Ok(r)
    }

    async fn state(&self) -> ActiveState {
        self.recipe_service_read().await.state().await
    }

    async fn start_recipe(
        &self,
        id: RecipeId,
        options: StartRecipeOptions,
    ) -> Result<(), TransactionError> {
        self.start_recipe_anyhow(id, options)
            .await
            // Errors of the recipe change are `TransactionError`s already: Don't wrap them in `Other` again
            .map_err(|e| {
                e.downcast::<TransactionError>()
                    .unwrap_or_else(TransactionError::Other)
            })
    }

    async fn update_device_params(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        values: ParameterUpdate,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        let mut s = self.recipe_service_write().await;
        s.update_device_params(recipe_id, device_id, values, &options)
            .await?;
        s.commit(options.key).await?;
        Ok(())
    }

    async fn commit_active(&self, transaction_key: Uuid) -> Result<(), TransactionError> {
        let mut s = self.recipe_service_write().await;
        s.commit_active().await?;
        s.commit(transaction_key).await?;
        Ok(())
    }

    async fn delete_device(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        let mut s = self.recipe_service.write().await;
        s.delete_device(recipe_id, device_id).await?;
        s.commit(options.key).await?;
        Ok(())
    }

    async fn restore_committed(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        transaction_key: Uuid,
    ) -> Result<(), TransactionError> {
        let mut s = self.recipe_service_write().await;
        s.restore_committed(recipe_id, device_id).await?;
        s.commit(transaction_key).await?;
        Ok(())
    }

    async fn update_device_name(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        name: Name,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        let mut s = self.recipe_service_write().await;
        s.update_device_name(recipe_id, device_id, name).await?;
        s.commit(options.key).await?;
        Ok(())
    }

    fn get_update_receiver(&self) -> BoxStream<'static, Uuid> {
        self.recipe_service.get_update_receiver()
    }
}

#[cfg(any(test, feature = "unstable"))]
pub(crate) mod unstable {
    use super::builder::RecipeServiceImplBuilder;
    use crate::recipe::{RecipeImporterImpl, RecipeServiceBuilder};
    use pilatus::{DeviceConfig, FileService, RecipeImporter, RelativeResourcePath};

    use super::*;
    use std::{path::PathBuf, sync::Arc};
    impl RecipeServiceImpl {
        pub fn create_temp_builder() -> (tempfile::TempDir, RecipeServiceImplBuilder) {
            let dir = tempfile::tempdir().unwrap();
            let rs = RecipeServiceImplBuilder {
                recipe_builder: RecipeServiceBuilder::new(
                    dir.path(),
                    Arc::new(super::super::parameters::LambdaRecipePermissioner::always_ok()),
                ),
            };
            (dir, rs)
        }
        pub fn device_dir(&self, device_id: &DeviceId) -> PathBuf {
            self.recipe_dir_path().join(device_id.to_string())
        }

        pub async fn device_config(
            &self,
            recipe_id: RecipeId,
            device_id: DeviceId,
        ) -> Result<DeviceConfig, TransactionError> {
            self.recipe_service_read()
                .await
                .device_config(recipe_id, device_id)
        }

        pub async fn add_recipe_with_id(
            &self,
            id: RecipeId,
            recipe: Recipe,
        ) -> Result<(), TransactionError> {
            let mut s = self.recipe_service_write().await;
            s.add_recipe_with_id(id, recipe).await?;
            s.commit(Uuid::new_v4()).await?;
            Ok(())
        }

        pub async fn get_active_id(&self) -> RecipeId {
            self.recipe_service_read().await.get_active_id()
        }

        pub fn build_device_file_service(&self, device_id: DeviceId) -> FileService<()> {
            self.build_file_service().build(device_id)
        }

        pub async fn add_recipe(&self, r: Recipe) -> Result<RecipeId, TransactionError> {
            let mut s = self.recipe_service_write().await;
            let r = s.add_recipe(r).await?;
            s.commit(Uuid::new_v4()).await?;
            Ok(r)
        }

        pub async fn add_device_with_id(
            &self,
            recipe_id: RecipeId,
            id: DeviceId,
            device: DeviceConfig,
        ) -> Result<(), TransactionError> {
            let mut s = self.recipe_service_write().await;
            s.add_device_with_id(recipe_id, id, device).await?;
            s.commit(Uuid::new_v4()).await?;
            Ok(())
        }

        pub async fn add_device_to_active_recipe(
            &self,
            device: DeviceConfig,
        ) -> Result<DeviceId, TransactionError> {
            let mut s = self.recipe_service_write().await;
            let r = s.add_device_to_active_recipe(device).await?;
            s.commit(Uuid::new_v4()).await?;
            Ok(r)
        }

        pub async fn add_device_to_recipe(
            &self,
            recipe_id: RecipeId,
            device: DeviceConfig,
        ) -> Result<DeviceId, TransactionError> {
            let mut s = self.recipe_service_write().await;
            let r = s.add_device_to_recipe(recipe_id, device).await?;
            s.commit(Uuid::new_v4()).await?;
            Ok(r)
        }
        pub fn create_importer(&self) -> RecipeImporter {
            Box::new(RecipeImporterImpl(Arc::new(self.clone())))
        }

        pub async fn create_device_file(&self, did: DeviceId, filename: &str, content: &[u8]) {
            let service = self.build_device_file_service(did);
            service
                .add_file_unchecked(&RelativeResourcePath::new(filename).unwrap(), content)
                .await
                .unwrap();
        }
    }
}
