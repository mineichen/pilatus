use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use minfac::{Registered, ServiceCollection};
use pilatus::device::ActiveState;
use pilatus::{
    device::DeviceId, Name, ParameterUpdate, Recipe, RecipeId, RecipeMetadata, RecipeService,
    RecipeServiceTrait, TransactionError, TransactionOptions,
};
use pilatus::{FileServiceBuilder, RecipeExporter, RecipeImporter};
use tokio::sync::{RwLockReadGuard, RwLockWriteGuard};
use uuid::Uuid;

use crate::TokioFileService;

use super::{RecipeDataService, RecipeImporterImpl, RecipeServiceAccessor};

mod builder;

pub use builder::*;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<Registered<Arc<RecipeServiceAccessor>>>()
        .register(|recipe_service| Arc::new(RecipeServiceFassade { recipe_service }))
        .alias(|x| x as RecipeService);
    c.with::<Registered<Arc<RecipeServiceFassade>>>()
        .register(|x| x as RecipeExporter);

    c.with::<Registered<Arc<RecipeServiceFassade>>>()
        .register(|x| Box::new(RecipeImporterImpl(x)) as RecipeImporter);
}

#[derive(Clone, Debug)]
pub struct RecipeServiceFassade {
    recipe_service: Arc<RecipeServiceAccessor>,
}

impl RecipeServiceFassade {
    pub(super) fn recipe_service(&self) -> &RecipeServiceAccessor {
        &self.recipe_service
    }
    pub async fn recipe_service_read(
        &self,
    ) -> RecipeDataService<RwLockReadGuard<pilatus::Recipes>> {
        self.recipe_service.read().await
    }

    pub async fn recipe_service_write(
        &self,
    ) -> RecipeDataService<RwLockWriteGuard<pilatus::Recipes>> {
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
impl RecipeServiceTrait for RecipeServiceFassade {
    async fn add_new_default_recipe_with(
        &self,
        options: TransactionOptions,
    ) -> Result<(RecipeId, Recipe), TransactionError> {
        let mut s = self.recipe_service_write().await;
        let r = s.add_new_default_recipe().await?;
        s.commit(options.key).await?;
        Ok(r)
    }

    async fn update_recipe_metadata_with(
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

    async fn delete_recipe_with(
        &self,
        recipe_id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        let mut s = self.recipe_service_write().await;
        s.delete_recipe(recipe_id).await?;
        s.commit(options.key).await?;
        Ok(())
    }

    async fn duplicate_recipe_with(
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

    async fn activate_recipe_with(
        &self,
        id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        let mut s = self.recipe_service_write().await;
        s.activate_recipe(id).await?;
        s.commit(options.key).await?;
        Ok(())
    }

    async fn update_device_params_with(
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

    async fn restore_active_with(&self, transaction_key: Uuid) -> Result<(), TransactionError> {
        let mut s = self.recipe_service_write().await;
        s.restore_active().await?;
        s.commit(transaction_key).await?;
        Ok(())
    }

    async fn commit_active_with(&self, transaction_key: Uuid) -> Result<(), TransactionError> {
        let mut s = self.recipe_service_write().await;
        s.commit_active().await?;
        s.commit(transaction_key).await?;
        Ok(())
    }

    async fn delete_device_with(
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

    async fn update_device_name_with(
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
    use pilatus::{DeviceConfig, FileService, RecipeImporter};

    use crate::recipe::{RecipeImporterImpl, RecipeServiceBuilder};

    use super::*;
    use std::{path::PathBuf, sync::Arc};
    impl RecipeServiceFassade {
        pub fn create_temp_builder() -> (tempfile::TempDir, RecipeServiceFassadeBuilder) {
            let dir = tempfile::tempdir().unwrap();
            let rs = RecipeServiceFassadeBuilder {
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
            let mut service = self.build_device_file_service(did);
            service
                .add_file_unchecked(&filename.try_into().unwrap(), content)
                .await
                .unwrap();
        }
    }
}
