use futures::{stream::FuturesUnordered, StreamExt};
use minfac::{Resolvable, ServiceCollection, ServiceProvider};
use std::{
    any::Any,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::runtime::Builder;
use tracing::{error, info};

use pilatus::{GenericConfig, HostedService, SystemTerminator};
use serde_json::Value as JsonValue;

use crate::metadata_future::MetadataFuture;

pub struct Runtime {
    services: ServiceCollection,
    #[cfg(feature = "tracing")]
    tracing: bool,
}

/// Convenience wrapper for integration tests.
/// Keeps the temporary directory alive for as long as the runtime is used.
pub struct TempRuntime {
    config_json: Option<JsonValue>,
    steps: Vec<TempRuntimeStep>,
}

enum TempRuntimeStep {
    Registrar(extern "C" fn(&mut ServiceCollection)),
    Instance(Box<dyn FnOnce(&mut ServiceCollection)>),
}

impl TempRuntime {
    /// Creates a temp runtime builder. No IO happens until [`TempRuntime::configure`].
    pub fn new() -> Self {
        Self {
            config_json: None,
            steps: Vec::new(),
        }
    }

    /// Sets the `config.json` contents to be written during [`TempRuntime::configure`].
    pub fn config(mut self, config_json: JsonValue) -> Self {
        self.config_json = Some(config_json);
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

    /// Like [`TempRuntime::run_until`], but for a pre-configured runtime.
    pub fn run_until<T, TDeps, TFut, F>(self, f: F) -> T
    where
        TDeps: Resolvable,
        TFut: futures::Future<Output = T>,
        F: FnOnce(TDeps) -> TFut,
    {
        let deps = self
            .inner
            .provider
            .resolve::<TDeps>()
            .expect("Missing dependencies for TempConfiguredRuntime::run_until");

        self.inner.run_until_finished(f(deps))
    }

    pub fn run(self, other: impl futures::Future<Output = ()>) {
        self.inner.run(other);
    }
}

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
    pub fn run(self) {
        self.configure().run(async {})
    }
}

pub struct ConfiguredRuntime {
    tokio: Arc<tokio::runtime::Runtime>,
    pub provider: ServiceProvider,
}

impl ConfiguredRuntime {
    pub fn run_until_finished<TFut: futures::Future>(self, other: TFut) -> TFut::Output {
        let terminator = self
            .provider
            .get::<SystemTerminator>()
            .expect("Cannot create Runtime without create::register, which provides this type");
        self.run_and_return(async move {
            let r = other.await;
            terminator.shutdown();
            r
        })
    }

    pub fn run(self, other: impl futures::Future<Output = ()>) {
        self.run_and_return(other);
    }

    fn run_and_return<TFut: futures::Future>(self, other: TFut) -> TFut::Output {
        info!("Tokio runtime has started.");
        let (r, _) = self.tokio.block_on(futures::future::join(other, async {
            let mut tasks: FuturesUnordered<_> = self
                .provider
                .get_all::<HostedService>()
                .filter_map(|i| match i.call((&self.provider).into()) {
                    Ok(x) => {
                        let name = i.get_name().to_string();
                        Some(MetadataFuture::new(name, x))
                    }
                    Err(e) => {
                        error!("Failed to call HostedService: {}", e);
                        None
                    }
                })
                .collect();
            while let Some((name, finished)) = tasks.next().await {
                let flattened = finished.map_err(anyhow::Error::from).and_then(|e| e);
                match flattened {
                    Ok(_) => {
                        let mut remaining_tasks = tasks.iter();
                        if let (Some(x), None) = (remaining_tasks.next(),remaining_tasks.next()) {
                           let remaining_name = x.get_meta();
                            info!(
                                "HostedService '{name}' stopped. Just service '{remaining_name}' is remaining"
                            );
                        } else {
                            let count = tasks.len();
                            info!("HostedService '{name}' stopped. '{count}' remaining");    
                        }
                    }
                    Err(e) => {
                        error!("Hosted service '{name}' failed: {e:?}");
                    }
                }
            }
        }));

        info!("Tokio runtime has ended.");
        r
    }
}
