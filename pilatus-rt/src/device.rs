use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::anyhow;
use async_trait::async_trait;
use futures::{
    channel::oneshot::{self, Sender},
    future::select_all,
    FutureExt, TryFutureExt,
};
use minfac::{AllRegistered, Registered, ServiceCollection, WeakServiceProvider};
use pilatus::device::DeviceContext;
use pilatus::TransactionOptions;
use pilatus::Variables;
use pilatus::{
    device::{ActorSystem, DeviceId, FinalizeRecipeExecution, RecipeRunner, RecipeRunnerTrait},
    prelude::*,
    DeviceConfig, RecipeId, RecipeServiceTrait, SystemShutdown,
};
use tracing::{error, info};

use crate::recipe::StartDeviceError;
use crate::recipe::{DeviceSpawnerService, RecipeServiceImpl};

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<(
        Registered<RecipeRunnerImpl>,
        Registered<Arc<RecipeServiceImpl>>,
        Registered<ActorSystem>,
        Registered<SystemShutdown>,
    )>()
    .register_hosted_service("Device Runner", run_devices_from_service);

    c.register_shared(|| Arc::new(RecipeRunnerState::default()));

    c.with::<(
        WeakServiceProvider,
        Registered<Arc<RecipeRunnerState>>,
        Registered<DeviceSpawnerService>,
        AllRegistered<Arc<dyn FinalizeRecipeExecution>>,
    )>()
    .register(|(provider, state, spawner, finalizer)| {
        RecipeRunnerImpl::new(provider, state, spawner, finalizer.collect())
    });
    c.with::<(Registered<RecipeRunnerImpl>, Registered<ActorSystem>)>()
        .register(|(recipe_runner, actor_system)| {
            RecipeRunner::new(Arc::new(RecipeRunnerService {
                recipe_runner,
                actor_system,
            }))
        });
}

type RunJob = Sender<(RecipeId, Sender<anyhow::Result<()>>)>;

async fn run_devices_from_service(
    (runner, recipe_service, actor_system, shutdown): (
        RecipeRunnerImpl,
        Arc<RecipeServiceImpl>,
        ActorSystem,
        SystemShutdown,
    ),
) -> Result<(), anyhow::Error> {
    let (r1, r2) = tokio::join!(runner.run_active_recipe(&recipe_service), async {
        shutdown.await;
        runner.set_next(None)?;
        actor_system.forget_senders();
        anyhow::Result::<()>::Ok(())
    });

    r1.and(r2)
}

#[derive(Clone)]
pub struct RecipeRunnerImpl {
    provider: WeakServiceProvider,
    state: Arc<RecipeRunnerState>,
    spawner: DeviceSpawnerService,
    finalizer: Vec<Arc<dyn FinalizeRecipeExecution>>,
}

struct RecipeRunnerService {
    recipe_runner: RecipeRunnerImpl,
    actor_system: ActorSystem,
}

#[async_trait]
impl RecipeRunnerTrait for RecipeRunnerService {
    async fn select_recipe(&self, recipe_id: RecipeId) -> anyhow::Result<()> {
        let result = self.recipe_runner.select_recipe(recipe_id)?;
        self.actor_system.forget_senders();
        result.await
    }
}

impl RecipeRunnerImpl {
    fn select_recipe(
        &self,
        recipe_id: RecipeId,
    ) -> anyhow::Result<impl Future<Output = anyhow::Result<()>>> {
        let sender = {
            self.state
                .next_recipe_id
                .lock()
                .expect("Not poisoned")
                .take()
        }
        .ok_or_else(|| anyhow::anyhow!("Not ready to select new RecipeId"))?;

        let (tx, rx) = oneshot::channel();
        sender
            .send((recipe_id, tx))
            .map_err(|_| anyhow::anyhow!("Couldn't send Uuid to channel"))?;
        Ok(rx
            .map_err(Into::into)
            .and_then(|x| async move { x })
            .boxed())
    }

    fn new(
        provider: WeakServiceProvider,
        state: Arc<RecipeRunnerState>,
        spawner: DeviceSpawnerService,
        finalizer: Vec<Arc<dyn FinalizeRecipeExecution>>,
    ) -> Self {
        Self {
            provider,
            state,
            spawner,
            finalizer,
        }
    }

    fn set_next(&self, n: Option<RunJob>) -> anyhow::Result<()> {
        let mut next = self
            .state
            .next_recipe_id
            .lock()
            .map_err(|e| anyhow!("Lock was poisoned: {e}"))?;
        *next = n;
        Ok(())
    }

    async fn run_active_recipe(
        &self,
        recipe_service: &RecipeServiceImpl,
    ) -> Result<(), anyhow::Error> {
        loop {
            let (_recipe_id, active_devices, variables) =
                recipe_service.get_owned_devices_from_active().await;
            let (tx, rx) = oneshot::channel();
            // Allow new recipe via self.select_recipe()
            *self.state.next_recipe_id.lock().expect("Not poisoned") = Some(tx);
            self.run_devices(
                active_devices,
                variables,
                |info| info!(info),
                |error| error!(error),
            )
            .await?;

            futures::future::join_all(self.finalizer.iter().map(|x| x.finalize_recipe_execution()))
                .await;

            match rx.await {
                Ok((next_id, select_recipe_response)) => {
                    let _ignore_absent_receiver = select_recipe_response.send(
                        recipe_service
                            .set_recipe_to_active(next_id, TransactionOptions::default())
                            .await
                            .map_err(Into::into),
                    );
                }
                Err(_) => break,
            }
        }
        Ok(())
    }
    async fn run_devices(
        &self,
        active_devices: impl IntoIterator<Item = (DeviceId, DeviceConfig)>,
        variables: Variables,
        mut info_logger: impl FnMut(String),
        mut error_logger: impl FnMut(String),
    ) -> Result<(), anyhow::Error> {
        let mut device_futures = Vec::new();

        for (id, device) in active_devices {
            let device_type = device.get_device_type().to_string();

            match self
                .spawner
                .spawn(
                    &device_type,
                    DeviceContext::new(id, variables.clone(), device.params.clone()),
                    self.provider.clone(),
                )
                .await
            {
                Ok(x) => {
                    info!("Starting Device '{device_type}' with id '{id}'");
                    device_futures.push(crate::MetadataFuture::new((id, device_type), x));
                }
                Err(StartDeviceError::UnknownDeviceType) => {
                    error!(device = device.get_device_type(), "Unknown DeviceType");
                }
                Err(StartDeviceError::Validation(e)) => {
                    error!(message = %e, "Invalid Params for Device '{device_type}' with id '{id}'");
                }
                Err(StartDeviceError::Io(e)) => {
                    error!(message = %e, "Couldn't spawn Device '{device_type}' with id '{id}'");
                }
            }
        }

        while !device_futures.is_empty() {
            let (((id, devicetype), finished), _, rest) = select_all(device_futures).await;
            device_futures = rest;
            let flattened = finished.map_err(anyhow::Error::from).and_then(|e| e);
            if let Err(e) = flattened {
                for cause in e.chain() {
                    (error_logger)(format!(
                        "Error in actor {} of type {}: {:?}",
                        id, devicetype, cause
                    ));
                }
            } else {
                (info_logger)(format!(
                    "Device {id} of Type '{devicetype}' stopped, {}",
                    if device_futures.len() == 1 {
                        format!("1 remaining ({:?})", device_futures[0].get_meta())
                    } else {
                        format!("{} remaining", device_futures.len())
                    }
                ));
            }
        }

        Ok(())
    }
}

#[derive(Default)]
struct RecipeRunnerState {
    next_recipe_id: Mutex<Option<RunJob>>,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use pilatus::{device::DeviceValidationContext, Name, UpdateParamsMessageError};

    async fn validate_ok(
        _ctx: DeviceValidationContext<'_>,
    ) -> Result<(), UpdateParamsMessageError> {
        Ok(())
    }

    #[tokio::test]
    async fn logs_correct_device() {
        let mut collection = minfac::ServiceCollection::new();
        collection
            .with::<()>()
            .register_device("foo", validate_ok, |_, _, _| async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                Ok(())
            });
        collection
            .with::<()>()
            .register_device("bar", validate_ok, |_, _, _| async {
                tokio::time::sleep(Duration::from_millis(1)).await;
                Ok(())
            });
        collection
            .with::<()>()
            .register_device("baz", validate_ok, |_, _, _| async {
                tokio::time::sleep(Duration::from_millis(40)).await;
                Ok(())
            });
        let mut messages = Vec::new();
        let messages_ref = &mut messages;
        let mut errors = Vec::new();
        let errors_ref = &mut errors;
        let provider = collection.build().unwrap();
        let weak_provider: WeakServiceProvider = (&provider).into();
        let state = RecipeRunnerState::default();
        let runner = RecipeRunnerImpl::new(
            weak_provider,
            Arc::new(state),
            DeviceSpawnerService::new(provider.get_all()),
            Vec::new(),
        );
        runner
            .run_devices(
                [
                    (
                        DeviceId::new_v4(),
                        DeviceConfig::new("foo", Name::new("MyFoo").unwrap(), "{}"),
                    ),
                    (
                        DeviceId::new_v4(),
                        DeviceConfig::new("bar", Name::new("MyBar").unwrap(), "{}"),
                    ),
                    (
                        DeviceId::new_v4(),
                        DeviceConfig::new("baz", Name::new("MyBaz").unwrap(), "{}"),
                    ),
                ]
                .into_iter()
                .collect::<Vec<_>>(),
                Variables::default(),
                move |x| messages_ref.push(x),
                move |x| errors_ref.push(x),
            )
            .await
            .unwrap();

        let mut messages_iter = messages.into_iter();
        let bar_msg = messages_iter.next().expect("has message for bar");
        assert!(
            bar_msg.contains("Type 'bar'"),
            "'{bar_msg}' doesn't contain 'bar'"
        );

        let foo_msg = messages_iter.next().expect("has message for foo");
        assert!(
            foo_msg.contains("Type 'foo'"),
            "'{foo_msg}' doesn't contain 'foo'"
        );
        assert!(
            foo_msg.contains("baz\")"),
            "'{foo_msg}' doesn't contain '(baz)'"
        );
        let baz_msg = messages_iter.next().expect("has message for foo");
        assert!(
            baz_msg.contains("Type 'baz'"),
            "'{baz_msg}' doesn't contain 'baz'"
        );
    }
}
