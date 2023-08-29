use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use minfac::{Registered, ServiceCollection};
use pilatus::device::ActiveState;
use pilatus::{
    device::DeviceId, Name, ParameterUpdate, Recipe, RecipeId, RecipeMetadata, RecipeService,
    RecipeServiceTrait, TransactionError, TransactionOptions,
};
use uuid::Uuid;

use super::RecipeServiceImpl;

mod builder;

pub use builder::*;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<Registered<Arc<RecipeServiceImpl>>>()
        .register(|recipe_service| Arc::new(RecipeServiceFassade { recipe_service }))
        .alias(|x| x as RecipeService);
}

#[cfg(any(test, feature = "unstable"))]
pub(crate) mod unstable {
    use pilatus::{DeviceConfig, FileService, RecipeImporter};

    use crate::recipe::{RecipeImporterImpl, RecipeServiceBuilder};

    use super::*;
    use std::sync::Arc;
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

        pub async fn add_recipe_with_id(
            &self,
            id: RecipeId,
            recipe: Recipe,
        ) -> Result<(), TransactionError> {
            self.recipe_service
                .add_recipe_with_id(id, recipe, Default::default())
                .await
        }

        pub async fn get_active_id(&self) -> RecipeId {
            self.recipe_service.get_active_id().await
        }

        pub fn build_device_file_service(&self, id: DeviceId) -> FileService<()> {
            self.recipe_service.build_device_file_service(id)
        }

        pub async fn add_recipe(&self, r: Recipe) -> Result<RecipeId, TransactionError> {
            self.recipe_service.add_recipe(r, Default::default()).await
        }

        pub async fn add_device_with_id(
            &self,
            recipe_id: RecipeId,
            id: DeviceId,
            device: DeviceConfig,
        ) -> Result<(), TransactionError> {
            self.recipe_service
                .add_device_with_id(recipe_id, id, device)
                .await
        }

        pub async fn add_device_to_active_recipe(
            &self,
            device: DeviceConfig,
        ) -> Result<DeviceId, TransactionError> {
            self.recipe_service
                .add_device_to_active_recipe(device, Default::default())
                .await
        }

        pub fn create_importer(&self) -> RecipeImporter {
            Box::new(RecipeImporterImpl(self.recipe_service.clone()))
        }

        pub async fn create_device_file(&self, did: DeviceId, filename: &str, content: &[u8]) {
            let mut service = self.build_device_file_service(did);
            service
                .add_file_unchecked(&filename.try_into().unwrap(), content)
                .await
                .unwrap();
        }

        pub async fn add_device_to_recipe(
            &self,
            recipe_id: RecipeId,
            device: DeviceConfig,
        ) -> Result<DeviceId, TransactionError> {
            self.recipe_service
                .add_device_to_recipe(recipe_id, device, Default::default())
                .await
        }
    }
}
#[cfg(any(test, feature = "unstable"))]
pub use unstable::*;

#[async_trait]
impl RecipeServiceTrait for RecipeServiceFassade {
    async fn add_new_default_recipe(
        &self,
        options: TransactionOptions,
    ) -> Result<(RecipeId, Recipe), TransactionError> {
        self.recipe_service.add_new_default_recipe(options).await
    }

    async fn update_recipe_metadata(
        &self,
        id: RecipeId,
        data: RecipeMetadata,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        self.recipe_service
            .update_recipe_metadata(id, data, options)
            .await
    }

    async fn delete_recipe_with(
        &self,
        recipe_id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        self.recipe_service.delete_recipe(recipe_id, options).await
    }

    async fn clone_recipe(
        &self,
        recipe_id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(RecipeId, Recipe), TransactionError> {
        self.recipe_service.clone_recipe(recipe_id, options).await
    }

    async fn state(&self) -> ActiveState {
        self.recipe_service.state().await
    }

    async fn set_recipe_to_active(
        &self,
        id: RecipeId,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        self.recipe_service.set_recipe_to_active(id, options).await
    }

    async fn update_device_params(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        values: ParameterUpdate,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        self.recipe_service
            .update_device_params(recipe_id, device_id, values, options)
            .await
    }

    async fn restore_active(&self) -> Result<(), TransactionError> {
        self.recipe_service.restore_active().await
    }

    async fn commit_active(&self, transaction_key: Uuid) -> Result<(), TransactionError> {
        self.recipe_service.commit_active(transaction_key).await
    }

    async fn delete_device(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
    ) -> Result<(), TransactionError> {
        self.recipe_service
            .delete_device(recipe_id, device_id)
            .await
    }

    async fn restore_committed(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        transaction: Uuid,
    ) -> Result<(), TransactionError> {
        self.recipe_service
            .restore_committed(recipe_id, device_id, transaction)
            .await
    }

    async fn update_device_name(
        &self,
        recipe_id: RecipeId,
        device_id: DeviceId,
        name: Name,
        options: TransactionOptions,
    ) -> Result<(), TransactionError> {
        self.recipe_service
            .update_device_name(recipe_id, device_id, name, options)
            .await
    }

    fn get_update_receiver(&self) -> BoxStream<'static, Uuid> {
        self.recipe_service.get_update_receiver()
    }
}
