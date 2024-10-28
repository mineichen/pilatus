use futures::{stream::FuturesUnordered, StreamExt};
use minfac::{ServiceCollection, ServiceProvider};
use std::{path::PathBuf, sync::Arc};
use tokio::runtime::Builder;
use tracing::{error, info};

use pilatus::{GenericConfig, HostedService, SystemTerminator};

use crate::metadata_future::MetadataFuture;

pub struct Runtime {
    services: ServiceCollection,
    #[cfg(feature = "tracing")]
    tracing: bool,
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
