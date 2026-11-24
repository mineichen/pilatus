use std::{
    any::Any,
    fmt::{self, Debug, Display},
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
};

use minfac::{Resolvable, ServiceCollection, ServiceProvider};
use pilatus::{GenericConfig, Recipe, Recipes, SystemTerminator};
use serde_json::Value as JsonValue;
use tokio::runtime::Builder;
use tracing::info;

use hosted_services::HostedServices;
pub use hosted_services::HostedServicesError;

mod hosted_services;

pub struct Runtime {
    services: ServiceCollection,
    #[cfg(feature = "tracing")]
    tracing: bool,
}

/// Convenience wrapper for integration tests.
/// Keeps the temporary directory alive for as long as the runtime is used.
pub struct TempRuntime {
    config_json: Option<JsonValue>,
    recipes: Option<Recipes>,
    steps: Vec<TempRuntimeStep>,
}

enum TempRuntimeStep {
    Registrar(extern "C" fn(&mut ServiceCollection)),
    Instance(Box<dyn FnOnce(&mut ServiceCollection)>),
}

impl Default for TempRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl TempRuntime {
    /// Creates a temp runtime builder. No IO happens until [`TempRuntime::configure`].
    pub fn new() -> Self {
        Self {
            config_json: None,
            recipes: None,
            steps: Vec::new(),
        }
    }

    /// Sets the `config.json` contents to be written during [`TempRuntime::configure`].
    pub fn config(mut self, config_json: JsonValue) -> Self {
        self.config_json = Some(config_json);
        self
    }

    /// Sets the recipes to be written during [`TempRuntime::configure`].
    ///
    /// If unset, TempRuntime will not create any recipe file on disk.
    pub fn set_recipes(mut self, recipes: Recipes) -> Self {
        self.recipes = Some(recipes);
        self
    }

    /// Adds a recipe to the recipes to be written during [`TempRuntime::configure`].
    ///
    /// If recipes were not set yet, creates a new [`Recipes`] from the given [`Recipe`].
    pub fn add_recipe(mut self, recipe: Recipe) -> Self {
        match &mut self.recipes {
            Some(recipes) => {
                recipes.add_new(recipe);
            }
            None => {
                self.recipes = Some(Recipes::new_with_recipe(recipe));
            }
        }
        self
    }

    pub fn register(mut self, registrar: extern "C" fn(&mut ServiceCollection)) -> Self {
        self.steps.push(TempRuntimeStep::Registrar(registrar));
        self
    }

    pub fn register_instance<T>(mut self, instance: T) -> Self
    where
        T: Clone + Send + Sync + Any + 'static,
    {
        self.steps.push(TempRuntimeStep::Instance(Box::new(
            move |c: &mut ServiceCollection| c.register_instance(instance),
        )));
        self
    }

    /// Creates a temporary root directory, writes `config.json`, builds the runtime and applies registrations.
    ///
    /// This is the only fallible operation in the TempRuntime API.
    pub fn configure(self) -> Result<TempConfiguredRuntime, std::io::Error> {
        let dir = tempfile::tempdir()?;
        if let Some(cfg) = self.config_json {
            let cfg = serde_json::to_vec(&cfg)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            std::fs::write(dir.path().join("config.json"), cfg)?;
        }
        if let Some(recipes) = self.recipes {
            let recipes_dir = dir.path().join("recipes");
            std::fs::create_dir_all(&recipes_dir)?;
            recipes.store_sync(recipes_dir.join("recipes.json"))?;
        }

        let mut runtime = Runtime::with_root(dir.path());
        for step in self.steps {
            match step {
                TempRuntimeStep::Registrar(registrar) => (registrar)(&mut runtime.services),
                TempRuntimeStep::Instance(f) => (f)(&mut runtime.services),
            }
        }

        Ok(TempConfiguredRuntime {
            dir,
            inner: runtime.configure(),
        })
    }
}

pub struct TempConfiguredRuntime {
    dir: tempfile::TempDir,
    inner: ConfiguredRuntime,
}

impl TempConfiguredRuntime {
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Runs the runtime until the future returned by `f` finishes. Shuts the system down afterwards and returns once all [`HostedService`](pilatus::HostedService)s stopped.
    ///
    /// `f` is called once all [`HostedService`](pilatus::HostedService)s are ready (see [`pilatus::ReadySignal`]).
    /// Services which stop before they are ready aren't waited for.
    /// If a service neither signals nor stops, `f` is never called. Pending services are logged every 5s.
    ///
    /// Failing services don't stop the runtime. Their errors are logged immediately and returned once all services stopped.
    /// The caller chooses the error type `E` (e.g. `testresult::TestError` or `anyhow::Error`). It's converted from [`RunUntilError`].
    #[track_caller]
    pub fn run_until<T, E, TDeps, TFut, F>(self, f: F) -> Result<T, E>
    where
        TDeps: Resolvable,
        TFut: Future<Output = Result<T, E>>,
        F: FnOnce(TDeps) -> TFut,
        E: From<RunUntilError<E>>,
    {
        let deps = self
            .inner
            .provider
            .resolve::<TDeps>()
            .expect("Missing dependencies for TempConfiguredRuntime::run_until");

        self.inner.run_until_finished(async move { f(deps).await })
    }

    /// Runs the runtime until all [`HostedService`](pilatus::HostedService)s stopped. Unlike [`TempConfiguredRuntime::run_until`], `other` doesn't shut the system down.
    ///
    /// `other` is polled once all [`HostedService`](pilatus::HostedService)s are ready. Errors are reported like in [`TempConfiguredRuntime::run_until`].
    #[track_caller]
    pub fn run<T, E>(self, other: impl Future<Output = Result<T, E>>) -> Result<T, E>
    where
        E: From<RunUntilError<E>>,
    {
        self.inner.run(other)
    }
}

/// Error of the `run*` methods of the runtime, which is converted into the error type of the caller
///
/// `E` only needs to implement [`Debug`], as e.g. `testresult::TestError` doesn't implement [`Display`].
/// So [`Display`] uses the [`Debug`] representation of `E`.
#[derive(Debug)]
pub enum RunUntilError<E> {
    /// The future (or closure) failed. All [`HostedService`](pilatus::HostedService)s succeeded
    Until(E),
    /// The future succeeded, but [`HostedService`](pilatus::HostedService)s failed
    HostedService(HostedServicesError),
    /// The future and [`HostedService`](pilatus::HostedService)s failed. The failure of the services might have caused the failure of the future
    HostedServiceAndUntil(HostedServicesError, E),
}

impl<E: From<RunUntilError<E>>> RunUntilError<E> {
    /// Combines the results of the future and the HostedServices into the error type of the caller
    #[track_caller]
    fn combine<T>(until: Result<T, E>, services: Result<(), HostedServicesError>) -> Result<T, E> {
        let error = match (until, services) {
            (Ok(x), Ok(())) => return Ok(x),
            (Err(until), Ok(())) => Self::Until(until),
            (Ok(_), Err(services)) => Self::HostedService(services),
            (Err(until), Err(services)) => Self::HostedServiceAndUntil(services, until),
        };
        Err(error.into())
    }
}

impl<E: Debug> Display for RunUntilError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Until(until) => write!(f, "{until:?}"),
            Self::HostedService(services) => write!(f, "{services}"),
            Self::HostedServiceAndUntil(services, until) => write!(f, "{until:?}; {services}"),
        }
    }
}

impl<E: Debug> std::error::Error for RunUntilError<E> {}

impl Default for Runtime {
    fn default() -> Self {
        Self::with_root(std::env::var("PILATUSROOT").unwrap_or_else(|_| "data".into()))
    }
}

impl Runtime {
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        std::fs::create_dir_all(&root).unwrap_or_else(|_| panic!("Can't create root dir {root:?}"));
        let mut services = ServiceCollection::new();
        let settings = root.join("settings.json");
        let config = GenericConfig::new(root).expect("Invalid config");

        #[cfg(feature = "tracing")]
        let tracing = crate::tracing::pre_init(&config, &mut services);

        info!("Start pilatus within root '{:?}'", config.root);

        services.register_instance(config);
        services.register_instance(
            pilatus::Settings::new(settings).expect("Found invalid data in settings.json"),
        );
        pilatus::register(&mut services);
        crate::register(&mut services);

        Self {
            services,
            #[cfg(feature = "tracing")]
            tracing,
        }
    }

    pub fn register(mut self, registrar: extern "C" fn(&mut ServiceCollection)) -> Self {
        (registrar)(&mut self.services);
        self
    }

    pub fn register_instance(mut self, instance: impl Clone + Send + Sync + Any) -> Self {
        self.services.register_instance(instance);
        self
    }

    /// As long as there is no Dynamic Plugin System, this method is allowed to panic, as it's the outermost layer
    pub fn configure(mut self) -> ConfiguredRuntime {
        // Should help to detect blocking threads/deadlocks
        #[cfg(debug_assertions)]
        let mut tokio_builder = Builder::new_current_thread();
        #[cfg(not(debug_assertions))]
        let mut tokio_builder = Builder::new_multi_thread();

        let tokio = Arc::new(
            tokio_builder
                .thread_name("pilatus")
                .enable_all()
                .thread_stack_size(3 * 1024 * 1024)
                .build()
                .unwrap(),
        );
        self.services.register_instance(tokio.clone());
        let provider = self.services.build().expect("has all dependencies");

        // Tracing is initialized a second time to get default-loglevels from pilatus plugins
        // This is useful so plugins can define their minimal topic-severity to avoid polluting the logs from their dependency
        // e.g. tokio-tungstenite logs a lot of data
        // pre_init() allows logs during the ServiceCollection::register() phase
        #[cfg(feature = "tracing")]
        crate::tracing::init(&provider, self.tracing).expect("Error during tracing setup");

        ConfiguredRuntime { tokio, provider }
    }

    /// Runs until all [`HostedService`](pilatus::HostedService)s stopped. Failing services don't stop the runtime.
    /// Their errors are logged immediately and returned once all services stopped.
    pub fn run(self) -> Result<(), HostedServicesError> {
        let ((), services) = self.configure().run_and_return(async {});
        services
    }
}

pub struct ConfiguredRuntime {
    tokio: Arc<tokio::runtime::Runtime>,
    pub provider: ServiceProvider,
}

impl ConfiguredRuntime {
    /// Runs the runtime until `other` finishes. Shuts the system down afterwards and returns once all [`HostedService`](pilatus::HostedService)s stopped.
    ///
    /// `other` is polled once all [`HostedService`](pilatus::HostedService)s are ready (see [`pilatus::ReadySignal`]).
    /// Services which stop before they are ready aren't waited for.
    /// If a service neither signals nor stops, `other` is never polled. Pending services are logged every 5s.
    ///
    /// Failing services don't stop the runtime. Their errors are logged immediately and returned once all services stopped.
    /// The caller chooses the error type `E` (e.g. `testresult::TestError` or `anyhow::Error`). It's converted from [`RunUntilError`].
    #[track_caller]
    pub fn run_until_finished<T, E>(self, other: impl Future<Output = Result<T, E>>) -> Result<T, E>
    where
        E: From<RunUntilError<E>>,
    {
        let terminator = self
            .provider
            .get::<SystemTerminator>()
            .expect("Cannot create Runtime without create::register, which provides this type");
        let (until, services) = self.run_and_return(async move {
            let r = other.await;
            terminator.shutdown();
            r
        });
        RunUntilError::combine(until, services)
    }

    /// Runs the runtime until all [`HostedService`](pilatus::HostedService)s stopped. Unlike [`ConfiguredRuntime::run_until_finished`], `other` doesn't shut the system down.
    ///
    /// `other` is polled once all [`HostedService`](pilatus::HostedService)s are ready. Errors are reported like in [`ConfiguredRuntime::run_until_finished`].
    #[track_caller]
    pub fn run<T, E>(self, other: impl Future<Output = Result<T, E>>) -> Result<T, E>
    where
        E: From<RunUntilError<E>>,
    {
        let (until, services) = self.run_and_return(other);
        RunUntilError::combine(until, services)
    }

    /// Polls `other` once all services are ready and returns once `other` finished and all services stopped
    fn run_and_return<TFut: Future>(
        self,
        other: TFut,
    ) -> (TFut::Output, Result<(), HostedServicesError>) {
        info!("Tokio runtime has started.");
        let result = self.tokio.block_on(async {
            let mut services = HostedServices::spawn(&self.provider);
            services.wait_until_ready().await;
            futures::future::join(other, services.wait_until_stopped()).await
        });
        info!("Tokio runtime has ended.");
        result
    }
}
