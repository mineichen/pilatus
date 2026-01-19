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

use crate::metadata_future::MetadataFuture;

pub struct Runtime {
    services: ServiceCollection,
    #[cfg(feature = "tracing")]
    tracing: bool,
}

/// Convenience wrapper for integration tests.
///
/// Creates a temporary root directory and builds a [`Runtime`] from it.
/// Keeps the temporary directory alive for as long as the runtime is used.
pub struct TempRuntime {
    dir: tempfile::TempDir,
    runtime: Runtime,
}

impl TempRuntime {
    /// Creates a temporary root and writes a default empty `config.json` (`{}`).
    pub fn new() -> std::io::Result<Self> {
        let dir = tempfile::tempdir()?;
        let runtime = Runtime::with_root(dir.path());
        Ok(Self { dir, runtime })
    }

    /// Replaces `config.json` in the temp root and rebuilds the internal [`Runtime`].
    ///
    /// Call this **before** using [`TempRuntime::register`] / [`TempRuntime::register_instance`],
    /// otherwise you'll lose previously added registrations (since rebuilding recreates the service collection).
    pub fn config_json(mut self, config_json: impl AsRef<[u8]>) -> std::io::Result<Self> {
        std::fs::write(self.dir.path().join("config.json"), config_json)?;
        self.runtime = Runtime::with_root(self.dir.path());
        Ok(self)
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn register(self, registrar: extern "C" fn(&mut ServiceCollection)) -> Self {
        let Self { dir, runtime } = self;
        Self {
            dir,
            runtime: runtime.register(registrar),
        }
    }

    pub fn register_instance(self, instance: impl Clone + Send + Sync + Any) -> Self {
        let Self { dir, runtime } = self;
        Self {
            dir,
            runtime: runtime.register_instance(instance),
        }
    }

    pub fn configure(self) -> TempConfiguredRuntime {
        let Self { dir, runtime } = self;
        let configured = runtime.configure();
        TempConfiguredRuntime {
            dir,
            inner: configured,
        }
    }

    /// Runs the runtime until the provided future finishes, while injecting dependencies from minfac.
    ///
    /// Dependencies are inferred from the closure's argument type and resolved via `provider.resolve()`.
    /// This allows call sites to avoid turbofish and only specify dependencies in the closure pattern:
    ///
    /// - `temp.run_until(|Registered(actor_system): Registered<ActorSystem>| async move { ... })`
    /// - `temp.run_until(|(Registered(stats), Registered(svc))| async move { ... })`
    pub fn run_until<T, TDeps, TFut, F>(self, f: F) -> T
    where
        TDeps: Resolvable,
        TFut: futures::Future<Output = T>,
        F: FnOnce(TDeps) -> TFut,
    {
        self.configure().run_until(f)
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
