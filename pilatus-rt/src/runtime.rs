use futures::{stream::FuturesUnordered, StreamExt};
use minfac::{ServiceCollection, ServiceProvider};
use std::{path::PathBuf, sync::Arc};
use tokio::runtime::Builder;
use tracing::{error, info};

use pilatus::{GenericConfig, HostedService};

use crate::metadata_future::MetadataFuture;

use super::occurance_counter::OccuranceCounter;

pub struct Runtime {
    services: ServiceCollection,
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
        crate::tracing::pre_init(&config, &mut services).expect("Should be able to setup logging");
        info!("Start pilatus within root '{:?}'", config.root);

        services.register_instance(config);
        services.register_instance(
            pilatus::Settings::new(settings).expect("Found invalid data in settings.json"),
        );
        pilatus::register(&mut services);
        crate::register(&mut services);

        Self { services }
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
        crate::tracing::init(&provider).expect("Error during tracing setup");

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
    pub fn run(self, other: impl futures::Future<Output = ()>) {
        info!("Tokio runtime has started.");
        self.tokio.block_on(futures::future::join(other, async {
            let (mut names, mut tasks): (OccuranceCounter<String>, FuturesUnordered<_>) = self
                .provider
                .get_all::<HostedService>()
                .filter_map(|i| match i.call((&self.provider).into()) {
                    Ok(x) => {
                        let name = i.get_name().to_string();
                        Some((name.clone(), MetadataFuture::new(name, x)))
                    }
                    Err(e) => {
                        error!("Failed to call HostedService: {}", e);
                        None
                    }
                })
                .unzip();
            while let Some((name, finished)) = tasks.next().await {
                let flattened = finished.map_err(anyhow::Error::from).and_then(|e| e);
                let is_removed = names.remove(&name);
                debug_assert!(is_removed, "Couldn't remove HostedService");
                match flattened {
                    Ok(_) => {
                        info!(
                            "HostedService '{name}' stopped. '{}' remaining",
                            names.len()
                        );
                    }
                    Err(e) => {
                        for cause in e.chain() {
                            error!("Handled on hosted service '{name}': {cause}");
                        }
                    }
                }
            }
        }));

        info!("Tokio runtime has ended.");
    }
}
