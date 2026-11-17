//! Tests how pending changes of the active recipe are resolved while the devices of a running app are replaced.
//! `RecipeRunner` is deprecated, but still part of the public API and covered here.
#![allow(deprecated)]

use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use minfac::{Registered, ServiceCollection};
use pilatus::device::{
    ActorResult, ActorSystem, DeviceContext, DeviceId, DeviceResult, DeviceValidationContext,
    RecipeRunner,
};
use pilatus::prelude::*;
use pilatus::{
    DeviceConfig, FileService, FileServiceBuilder, Name, ParameterUpdate, PendingChangesStrategy,
    Recipe, RecipeId, RecipeMetadataRaw, RecipeService, RelativeDirectoryPath,
    RelativeResourcePath, TransactionError, TransactionOptions, UncommittedChangesError,
    UntypedDeviceParamsWithVariables, UpdateParamsMessage, UpdateParamsMessageError,
};
use pilatus_rt::{RecipeServiceImpl, TempRuntime};

/// Records the params of every start and the number of stops of devices of type "counter"
#[derive(Default)]
struct DeviceLog {
    started: Mutex<Vec<String>>,
    stopped: AtomicUsize,
}

impl DeviceLog {
    fn started(&self) -> Vec<String> {
        self.started.lock().unwrap().clone()
    }

    fn stopped(&self) -> usize {
        self.stopped.load(Ordering::SeqCst)
    }

    async fn wait_for_starts(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.started().len() < count {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("Expected {count} starts, got {:?}", self.started()));
    }
}

struct Counter;

impl Counter {
    async fn update_params(
        &mut self,
        _msg: UpdateParamsMessage<String>,
    ) -> ActorResult<UpdateParamsMessage<String>> {
        Ok(())
    }
}

extern "C" fn register_counter_device(c: &mut ServiceCollection) {
    async fn validator(
        ctx: DeviceValidationContext<'_>,
    ) -> Result<String, UpdateParamsMessageError> {
        ctx.params_as::<String>()
    }

    async fn device(
        ctx: DeviceContext,
        params: String,
        (actor_system, log): (ActorSystem, Arc<DeviceLog>),
    ) -> DeviceResult {
        // Registered before the start is logged, so the device accepts messages as soon as a test sees it
        let actor = actor_system
            .register(ctx.id)
            .add_handler(Counter::update_params);
        log.started.lock().unwrap().push(params);
        // Runs until the senders are forgotten (cooperative shutdown)
        actor.execute(Counter).await;
        log.stopped.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    c.with::<(Registered<ActorSystem>, Registered<Arc<DeviceLog>>)>()
        .register_device("counter", validator, device);
}

fn counter(name: &str, params: &str) -> DeviceConfig {
    DeviceConfig::new_unchecked("counter", name, params)
}

/// A recipe with a single counter device, which is the active recipe of the app
fn recipe_with_counter(params: &str) -> (Recipe, DeviceId) {
    let mut recipe = Recipe::default();
    let device_id = recipe.add_device(counter("MyCounter", params));
    (recipe, device_id)
}

struct App {
    service: RecipeService,
    rs: Arc<RecipeServiceImpl>,
    files: FileServiceBuilder,
    log: Arc<DeviceLog>,
    root: PathBuf,
    legacy_runner: RecipeRunner,
}

impl App {
    fn files(&self, device_id: DeviceId) -> FileService {
        self.files.clone().build(device_id)
    }

    fn device_dir(&self, device_id: DeviceId) -> PathBuf {
        self.root.join("recipes").join(device_id.to_string())
    }

    async fn active_id(&self) -> RecipeId {
        self.service.state().await.recipes().active().0
    }

    async fn has_uncommitted_changes(&self) -> bool {
        self.service.state().await.has_uncommitted_changes()
    }

    async fn active_params(&self, device_id: DeviceId) -> anyhow::Result<String> {
        let state = self.service.state().await;
        let config = state.recipes().active().1.device_by_id(device_id)?;
        Ok(serde_json::from_value(serde_json::Value::clone(
            &config.params,
        ))?)
    }

    async fn set_params(&self, device_id: DeviceId, params: &str) -> anyhow::Result<()> {
        self.service
            .update_device_params(
                self.active_id().await,
                device_id,
                ParameterUpdate {
                    parameters: UntypedDeviceParamsWithVariables::from_serializable(params)?,
                    variables: Default::default(),
                },
            )
            .await?;
        Ok(())
    }

    async fn start_recipe(
        &self,
        id: &RecipeId,
        strategy: PendingChangesStrategy,
    ) -> Result<(), TransactionError> {
        // Right after a recipe was started, the runner needs a moment until it accepts the next request
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match self
                    .service
                    .start_recipe_with(id.clone(), strategy)
                    .await
                {
                    Err(TransactionError::Other(e)) if e.to_string().contains("Not ready") => {
                        tokio::time::sleep(Duration::from_millis(2)).await
                    }
                    result => return result,
                }
            }
        })
        .await
        .expect("The runner accepts the next recipe")
    }
}

/// Runs `test` within a started app, whose root directory and active recipe are created from scratch
fn run_app<Fut>(active_recipe: Recipe, test: impl FnOnce(App) -> Fut) -> anyhow::Result<()>
where
    Fut: Future<Output = anyhow::Result<()>>,
{
    let log = Arc::new(DeviceLog::default());
    let configured = TempRuntime::new()
        .add_recipe(active_recipe)
        .register(register_counter_device)
        .register_instance(log.clone())
        .configure()?;
    let root = configured.path().to_path_buf();

    configured.run_until(
        |(Registered(service), Registered(rs), Registered(files), Registered(legacy_runner)): (
            Registered<RecipeService>,
            Registered<Arc<RecipeServiceImpl>>,
            Registered<FileServiceBuilder>,
            Registered<RecipeRunner>,
        )| {
            test(App {
                service,
                rs,
                files,
                log,
                root,
                legacy_runner,
            })
        },
    )
}

#[test]
fn starting_the_active_recipe_always_restarts_devices() -> anyhow::Result<()> {
    let (recipe, device_id) = recipe_with_counter("a");
    run_app(recipe, |app| async move {
        let active_id = app.active_id().await;
        app.log.wait_for_starts(1).await;
        assert_eq!(0, app.log.stopped());

        // Nothing is pending, but the devices are restarted whatever the strategy is
        for (restarts, strategy) in [
            PendingChangesStrategy::Deny,
            PendingChangesStrategy::Commit,
            PendingChangesStrategy::Discard,
        ]
        .into_iter()
        .enumerate()
        {
            app.start_recipe(&active_id, strategy).await?;
            app.log.wait_for_starts(restarts + 2).await;
            assert_eq!(restarts + 1, app.log.stopped());
        }
        assert_eq!(vec!["a"; 4], app.log.started());

        // Pending changes with Deny (default) are refused, but the devices are restarted too (with the uncommitted state)
        app.set_params(device_id, "b").await?;
        assert!(app.has_uncommitted_changes().await);
        assert!(app
            .start_recipe(&active_id, PendingChangesStrategy::Deny)
            .await
            .is_err());
        app.log.wait_for_starts(5).await;
        assert_eq!(Some("b"), app.log.started().last().map(String::as_str));
        assert_eq!(4, app.log.stopped());
        assert!(app.has_uncommitted_changes().await);

        // Pending changes with Discard: The device is restarted with the committed params
        app.start_recipe(&active_id, PendingChangesStrategy::Discard)
            .await?;
        app.log.wait_for_starts(6).await;
        assert_eq!(Some("a"), app.log.started().last().map(String::as_str));
        assert_eq!(5, app.log.stopped());
        assert!(!app.has_uncommitted_changes().await);

        // Changed files are discarded while the devices are stopped
        app.files(device_id)
            .add_file_unchecked(RelativeResourcePath::new("recorded.txt")?, b"data")
            .await?;
        assert!(app.device_dir(device_id).join("recorded.txt").exists());
        assert!(app.has_uncommitted_changes().await);
        app.start_recipe(&active_id, PendingChangesStrategy::Discard)
            .await?;
        app.log.wait_for_starts(7).await;
        assert_eq!(6, app.log.stopped());
        assert!(!app.device_dir(device_id).join("recorded.txt").exists());
        assert!(!app.has_uncommitted_changes().await);

        // The deprecated RecipeRunner behaves like PendingChangesStrategy::Deny
        app.legacy_runner.select_recipe(active_id.clone()).await?;
        app.log.wait_for_starts(8).await;
        assert_eq!(7, app.log.stopped());
        app.set_params(device_id, "b").await?;
        assert!(app.legacy_runner.select_recipe(active_id).await.is_err());
        app.log.wait_for_starts(9).await;
        assert_eq!(8, app.log.stopped());
        Ok(())
    })
}

#[test]
fn pending_changes_are_denied_by_default() -> anyhow::Result<()> {
    let (recipe, did) = recipe_with_counter("a");
    run_app(recipe, |app| async move {
        let r2_id = app.rs.add_recipe(Recipe::default()).await?;
        app.set_params(did, "b").await?;

        assert!(app
            .start_recipe(&r2_id, PendingChangesStrategy::Deny)
            .await
            .is_err());
        // Nothing changed
        assert!(app.has_uncommitted_changes().await);
        assert_eq!("b", app.active_params(did).await?);

        // Also files are protected
        app.service.commit_active().await?;
        app.rs.create_device_file(did, "test.txt", b"test").await;
        assert!(app
            .start_recipe(&r2_id, PendingChangesStrategy::Deny)
            .await
            .is_err());
        assert!(app.device_dir(did).join("test.txt").exists());
        Ok(())
    })
}

#[test]
fn commit_strategy_keeps_changes() -> anyhow::Result<()> {
    let (recipe, did) = recipe_with_counter("a");
    run_app(recipe, |app| async move {
        let r1_id = app.active_id().await;
        let r2_id = app.rs.add_recipe(Recipe::default()).await?;
        app.set_params(did, "b").await?;
        app.rs.create_device_file(did, "test.txt", b"test").await;

        app.start_recipe(&r2_id, PendingChangesStrategy::Commit).await?;
        // No pending changes on the way back: Commit has really committed
        app.start_recipe(&r1_id, PendingChangesStrategy::Deny).await?;

        assert_eq!("b", app.active_params(did).await?);
        assert_eq!(
            1,
            app.files(did)
                .list_recursive(RelativeDirectoryPath::root())
                .await?
                .len()
        );
        Ok(())
    })
}

#[test]
fn discard_strategy_restores_active_before_switching() -> anyhow::Result<()> {
    let (recipe, did) = recipe_with_counter("a");
    run_app(recipe, |app| async move {
        let r1_id = app.active_id().await;
        app.rs
            .create_device_file(did, "original.txt", b"test")
            .await;
        app.service.commit_active().await?;
        let r2_id = app.rs.add_recipe(Recipe::default()).await?;

        app.set_params(did, "b").await?;
        app.rs.create_device_file(did, "new.txt", b"test").await;
        let new_device = app
            .rs
            .add_device_to_active_recipe(counter("NewCounter", "new"))
            .await?;
        app.rs
            .create_device_file(new_device, "test.txt", b"test")
            .await;

        app.start_recipe(&r2_id, PendingChangesStrategy::Discard)
            .await?;
        // The discarded recipe is clean, so it can be started without any strategy
        app.start_recipe(&r1_id, PendingChangesStrategy::Deny).await?;

        assert_eq!("a", app.active_params(did).await?);
        let state = app.service.state().await;
        assert!(!state.has_uncommitted_changes());
        assert_eq!(1, state.recipes().active().1.count_devices());
        let files = app
            .files(did)
            .list_recursive(RelativeDirectoryPath::root())
            .await?;
        assert_eq!(1, files.len());
        assert!(files[0].ends_with("original.txt"));
        assert!(!app.device_dir(new_device).exists());
        Ok(())
    })
}

#[test]
fn discard_recovers_deleted_device_with_files() -> anyhow::Result<()> {
    let (recipe, did) = recipe_with_counter("a");
    run_app(recipe, |app| async move {
        app.rs.create_device_file(did, "test.txt", b"test").await;
        app.service.commit_active().await?;

        let active_id = app.active_id().await;
        app.service.delete_device(active_id.clone(), did).await?;
        assert!(app.has_uncommitted_changes().await);

        app.start_recipe(&active_id, PendingChangesStrategy::Discard)
            .await?;
        assert!(!app.has_uncommitted_changes().await);
        assert_eq!(
            1,
            app.files(did)
                .list_recursive(RelativeDirectoryPath::root())
                .await?
                .len()
        );
        Ok(())
    })
}

#[test]
fn discard_reverts_params_and_added_devices() -> anyhow::Result<()> {
    let (recipe, did) = recipe_with_counter("a");
    run_app(recipe, |app| async move {
        assert!(!app.has_uncommitted_changes().await);

        app.set_params(did, "b").await?;
        app.rs
            .add_device_to_active_recipe(counter("NewCounter", "new"))
            .await?;
        assert!(app.has_uncommitted_changes().await);

        let active_id = app.active_id().await;
        app.start_recipe(&active_id, PendingChangesStrategy::Discard)
            .await?;
        let state = app.service.state().await;
        assert!(!state.has_uncommitted_changes());
        assert_eq!(1, state.recipes().active().1.count_devices());
        assert_eq!("a", app.active_params(did).await?);
        Ok(())
    })
}

#[test]
fn unknown_recipe_does_not_restart_devices() -> anyhow::Result<()> {
    let (recipe, _) = recipe_with_counter("a");
    run_app(recipe, |app| async move {
        app.log.wait_for_starts(1).await;
        let unknown_id = RecipeId::default().suggest_unique().next().unwrap();

        for strategy in [
            PendingChangesStrategy::Deny,
            PendingChangesStrategy::Commit,
            PendingChangesStrategy::Discard,
        ] {
            assert!(app.start_recipe(&unknown_id, strategy).await.is_err());
        }
        assert!(app.legacy_runner.select_recipe(unknown_id).await.is_err());

        // A device stops before the request is answered, so this is final
        assert_eq!(0, app.log.stopped());
        assert_eq!(vec!["a"], app.log.started());
        Ok(())
    })
}

#[test]
fn denied_changes_are_not_wrapped_twice() -> anyhow::Result<()> {
    let (recipe, did) = recipe_with_counter("a");
    run_app(recipe, |app| async move {
        let r2_id = app.rs.add_recipe(Recipe::default()).await?;
        app.set_params(did, "b").await?;

        let Err(TransactionError::Other(e)) =
            app.start_recipe(&r2_id, PendingChangesStrategy::Deny).await
        else {
            panic!("Expected TransactionError::Other");
        };
        assert!(e.is::<UncommittedChangesError>(), "{e:?}");
        assert_eq!("Uncommitted changes on active recipe", e.to_string());

        // The deprecated RecipeRunner reports the same error as before
        let legacy_error = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match app.legacy_runner.select_recipe(r2_id.clone()).await {
                    Err(e) if e.to_string().contains("Not ready") => {
                        tokio::time::sleep(Duration::from_millis(2)).await
                    }
                    result => break result.expect_err("Uncommitted changes are denied"),
                }
            }
        })
        .await?;
        assert_eq!(
            "Other: Uncommitted changes on active recipe",
            legacy_error.to_string()
        );
        Ok(())
    })
}

#[test]
fn tags_are_kept_and_id_of_active_recipe_cannot_change() -> anyhow::Result<()> {
    let (recipe, did) = recipe_with_counter("a");
    run_app(recipe, |app| async move {
        let active_id = app.active_id().await;
        let set_metadata = |new_id: &RecipeId, tag: &str| {
            let metadata = RecipeMetadataRaw {
                new_id: new_id.clone(),
                tags: vec![Name::new(tag).unwrap()],
            }
            .seal()
            .unwrap();
            app.service.update_recipe_metadata_with(
                active_id.clone(),
                metadata,
                TransactionOptions::default(),
            )
        };
        let active_tags = || async {
            let state = app.service.state().await;
            state
                .recipes()
                .get_with_id(&active_id)
                .unwrap()
                .tags
                .clone()
        };

        let renamed_id = active_id.suggest_unique().next().unwrap();
        assert!(set_metadata(&renamed_id, "foo").await.is_err());
        assert_eq!(active_id, app.active_id().await);

        set_metadata(&active_id, "foo").await?;
        assert!(!app.has_uncommitted_changes().await);

        for (strategy, tag) in [
            (PendingChangesStrategy::Discard, "bar"),
            (PendingChangesStrategy::Commit, "baz"),
        ] {
            app.set_params(did, "b").await?;
            set_metadata(&active_id, tag).await?;
            app.start_recipe(&active_id, strategy).await?;
            assert!(!app.has_uncommitted_changes().await);
            assert_eq!(vec![Name::new(tag)?], active_tags().await, "{strategy:?}");
        }
        Ok(())
    })
}
