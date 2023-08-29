use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use anyhow::anyhow;
use futures::future::BoxFuture;
use futures::FutureExt;
use minfac::{AllRegistered, Registered, ServiceCollection, WeakServiceProvider};
use tokio::task::JoinHandle;

use pilatus::device::{
    ActorSystem, DeviceContext, DeviceHandler, DeviceId, DeviceResult, UpdateDeviceError,
    WithInfallibleParamUpdate,
};
use pilatus::{TransactionError, TransactionOptions, UntypedDeviceParamsWithVariables};

use super::{ChangeDeviceParamsTransactionError, RecipeServiceBuilder, RecipeServiceImpl};

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<(
        AllRegistered<Box<dyn DeviceHandler>>,
        Registered<ActorSystem>,
    )>()
    .register(|(handlers, system)| DeviceSpawnerService::new(handlers, system));

    c.with::<Registered<DeviceSpawnerService>>()
        .register(|s| Arc::new(s) as Arc<dyn DeviceActions>);
}

impl DeviceActions for DeviceSpawnerService {
    fn validate(
        &self,
        device_type: &str,
        ctx: DeviceContext,
    ) -> BoxFuture<Result<WithInfallibleParamUpdate<()>, TransactionError>> {
        let spawner = self.get_spawner(device_type);
        async move { spawner?.validate(ctx).await.map_err(Into::into) }.boxed()
    }
    fn try_apply(
        &self,
        device_type: &str,
        ctx: DeviceContext,
    ) -> BoxFuture<Result<(), TransactionError>> {
        let spawner = self.get_spawner(device_type);
        async move {
            spawner?
                .update(ctx, self.actor_system.clone())
                .await
                .map_err(|e| match e {
                    UpdateDeviceError::Validate(x) => x.into(),
                    UpdateDeviceError::UnknownDevice(d) => d.into(),
                    UpdateDeviceError::Other(x) => x.into(),
                })
        }
        .boxed()
    }
}

#[derive(Clone)]
pub struct DeviceSpawnerService {
    actor_system: ActorSystem,
    map: HashMap<&'static str, Box<dyn DeviceHandler>>,
}

impl Debug for DeviceSpawnerService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActorSystemRecipePermissioner")
            .field("map", &self.map.keys())
            .finish()
    }
}

impl DeviceSpawnerService {
    pub fn new(
        devices: impl Iterator<Item = Box<dyn DeviceHandler>>,
        actor_system: ActorSystem,
    ) -> Self {
        Self {
            actor_system,
            map: devices.map(|d| (d.get_device_type(), d)).collect(),
        }
    }
    fn get_spawner(&self, device_type: &str) -> anyhow::Result<&dyn DeviceHandler> {
        self.map
            .get(device_type)
            .map(|f| f.as_ref())
            .ok_or_else(|| anyhow!("Unknown DeviceType {device_type}"))
    }
    pub fn spawn(
        &self,
        device_type: &str,
        ctx: DeviceContext,
        provider: WeakServiceProvider,
    ) -> BoxFuture<Result<WithInfallibleParamUpdate<JoinHandle<DeviceResult>>, StartDeviceError>>
    {
        let x = self
            .get_spawner(device_type)
            .map_err(|_| StartDeviceError::UnknownDeviceType);
        async move { Ok(x?.spawn(ctx, provider).await?) }.boxed()
    }
}
pub struct ChangeParamsStrategy {
    device_type: &'static str,
    type_id: std::any::TypeId,
    modifier: Box<dyn Any + Send + Sync>,
}

impl ChangeParamsStrategy {
    pub fn new<T: Any + Send + Sync>(
        device_type: &'static str,
        modifier: fn(
            &UntypedDeviceParamsWithVariables,
            T,
        ) -> Result<UntypedDeviceParamsWithVariables, TransactionError>,
    ) -> Self {
        Self {
            device_type,
            type_id: std::any::TypeId::of::<T>(),
            modifier: Box::new(modifier),
        }
    }
}

impl RecipeServiceImpl {
    pub async fn change_device_params_on_active_recipe<T: Any>(
        &self,
        device_id: DeviceId,
        msg: T,
        options: TransactionOptions,
    ) -> Result<(), ChangeDeviceParamsTransactionError> {
        let mut recipes = self.recipes.lock().await;

        let device = recipes.active().1.device_by_id(device_id)?;
        let modifier = self
            .change_strategies
            .get(&(&device.device_type, TypeId::of::<T>()))
            .ok_or(ChangeDeviceParamsTransactionError::UnknownModifier)?;

        let modifier = modifier
            .downcast_ref::<fn(
                &UntypedDeviceParamsWithVariables,
                T,
            ) -> Result<UntypedDeviceParamsWithVariables, TransactionError>>()
            .expect("Always true");

        let new_params = (modifier)(&device.params, msg)?;

        let variables = self
            .apply_params(device_id, &new_params, Default::default(), &recipes)
            .await?;

        options.update_device_params(recipes.get_active().1, device_id, new_params)?;

        *recipes.as_mut() = variables;
        self.save_config(&recipes, options.key).await?;
        Ok(())
    }
}

impl RecipeServiceBuilder {
    pub fn with_change_strategy(mut self, x: ChangeParamsStrategy) -> Self {
        self.change_strategies
            .insert((x.device_type, x.type_id), x.modifier);
        self
    }
}

#[cfg(any(test, feature = "unstable"))]
mod testutil {
    use std::fmt::Debug;

    use futures::{future::BoxFuture, FutureExt};

    use pilatus::{
        device::{DeviceContext, IntoParamValidatorOk, WithInfallibleParamUpdate},
        TransactionError,
    };

    pub struct LambdaRecipePermissioner<TValidator> {
        validator: TValidator,
    }

    impl<T> Debug for LambdaRecipePermissioner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("LambdaRecipePermissioner").finish()
        }
    }

    impl
        LambdaRecipePermissioner<
            fn() -> Result<WithInfallibleParamUpdate<()>, super::TransactionError>,
        >
    {
        pub fn always_ok() -> Self {
            LambdaRecipePermissioner {
                validator: || Ok(IntoParamValidatorOk::into_ok(())),
            }
        }
    }

    #[cfg(test)]
    impl<T: Fn() -> Result<WithInfallibleParamUpdate<()>, super::TransactionError>>
        LambdaRecipePermissioner<T>
    {
        pub fn with_validator(validator: T) -> Self {
            Self { validator }
        }
    }

    impl<
            T: Fn() -> Result<WithInfallibleParamUpdate<()>, super::TransactionError> + Send + Sync,
        > super::DeviceActions for LambdaRecipePermissioner<T>
    {
        fn validate(
            &self,
            _device_type: &str,
            _ctx: DeviceContext,
        ) -> BoxFuture<Result<WithInfallibleParamUpdate<()>, super::TransactionError>> {
            async { (self.validator)() }.boxed()
        }
        fn try_apply(
            &self,
            _device_type: &str,
            _ctx: DeviceContext,
        ) -> BoxFuture<Result<(), TransactionError>> {
            futures::future::ready(Ok(())).boxed()
        }
    }
}
#[cfg(any(test, feature = "unstable"))]
pub use testutil::*;

use super::actions::{DeviceActions, StartDeviceError};

#[cfg(test)]
mod tests {
    use pilatus::DeviceConfig;

    use crate::recipe::RecipeServiceFassade;

    use super::*;

    #[tokio::test]
    async fn change_device_params_on_active_recipe() -> anyhow::Result<()> {
        let (dir, rsb) = RecipeServiceFassade::create_temp_builder();
        let rs = rsb
            .with_change_strategy(ChangeParamsStrategy::new(
                "testdevice",
                |old_json, x: i32| {
                    old_json
                        .as_i64()
                        .expect("Expected to have a i64 in old params");
                    Ok(UntypedDeviceParamsWithVariables::from_serializable(x).unwrap())
                },
            ))
            .build();
        let recipe_id = rs.get_active_id().await;

        let device_id = rs
            .recipe_service
            .add_device_to_active_recipe(DeviceConfig::mock(1i32), Default::default())
            .await
            .unwrap();

        rs.recipe_service
            .change_device_params_on_active_recipe(device_id, 42i32, Default::default())
            .await
            .expect("Should be updateable");
        rs.recipe_service
            .change_device_params_on_active_recipe(device_id, 42u32, Default::default())
            .await
            .expect_err("Shouldn't be updateable");

        let config = rs
            .recipe_service
            .clone_device_config(recipe_id, device_id)
            .await
            .expect("Should have a device");
        assert_eq!(config, DeviceConfig::mock(42i32));

        dir.close()?;
        Ok(())
    }
}
