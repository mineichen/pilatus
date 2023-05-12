use futures::{stream::FuturesUnordered, StreamExt};
use minfac::{ServiceCollection, ServiceProvider};
use std::{path::PathBuf, sync::Arc};
use tokio::runtime::Builder;
use tracing::{error, info};

use pilatus::{GenericConfig, HostedService};

use super::occurance_counter::OccuranceCounter;

pub struct Runtime {
    #[cfg(feature = "tracing")]
    _trace_guard:
        Result<tracing_appender::non_blocking::WorkerGuard, tracing_subscriber::util::TryInitError>,
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
        let trace_guard = crate::tracing::init(&config);

        info!("Start pilatus within root '{:?}'", config.root);
        services.register_instance(config);
        services.register_instance(pilatus::Settings::new(settings).expect("Settings not found"));
        pilatus::register(&mut services);
        crate::register(&mut services);

        Self {
            services,
            #[cfg(feature = "tracing")]
            _trace_guard: trace_guard.map_err(|e| {
                eprintln!("Couldn't start tracing {e}");
                e
            }),
        }
    }

    pub fn register(mut self, registrar: extern "C" fn(&mut ServiceCollection)) -> Self {
        (registrar)(&mut self.services);
        self
    }

    /// As long as there is no Dynamic Plugin System, this method is allowed to panic, as it's the outermost layer
    pub fn configure(mut self) -> ConfiguredRuntime {
        #[cfg(feature = "leak-detect-allocator")]
        tracer::LEAK_TRACER.init();

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
        ConfiguredRuntime {
            #[cfg(feature = "tracing")]
            _trace_guard: self._trace_guard,
            tokio,
            provider,
        }
    }
    pub fn run(self) {
        self.configure().run(async {})
    }
}

pub struct ConfiguredRuntime {
    tokio: Arc<tokio::runtime::Runtime>,
    pub provider: ServiceProvider,
    #[cfg(feature = "tracing")]
    _trace_guard:
        Result<tracing_appender::non_blocking::WorkerGuard, tracing_subscriber::util::TryInitError>,
}

impl ConfiguredRuntime {
    pub fn run(self, other: impl futures::Future<Output = ()>) {
        info!("Tokio runtime has started.");
        self.tokio.block_on(futures::future::join(other, async {
            #[cfg(feature = "leak-detect-allocator")]
            tracer::spawn_leak_collector();

            let (mut names, mut tasks): (OccuranceCounter<String>, FuturesUnordered<_>) = self
                .provider
                .get_all::<HostedService>()
                .filter_map(|i| match i.call((&self.provider).into()) {
                    Ok(x) => {
                        let name = i.get_name().to_string();
                        Some((name.clone(), crate::MetadataFuture::new(name, x)))
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

#[cfg(feature = "leak-detect-allocator")]

mod tracer {
    use tracing::warn;

    #[global_allocator]
    static LEAK_TRACER: leak_detect_allocator::LeakTracerDefault =
        leak_detect_allocator::LeakTracerDefault::new();

    pub fn spawn_leak_collector() {
        tokio::spawn(async move {
            loop {
                let mut out = String::new();
                let mut count = 0;
                let mut count_size = 0;
                LEAK_TRACER.now_leaks(|address: usize, size: usize, stack: &[usize]| {
                    count += 1;
                    count_size += size;
                    out += &format!("leak memory address: {:#x}, size: {}\r\n", address, size);

                    for f in stack {
                        // Resolve this instruction pointer to a symbol name
                        out += &format!(
                            "\t{:#x}, {}\r\n",
                            *f,
                            LEAK_TRACER.get_symbol_name(*f).unwrap_or("".to_owned())
                        );
                    }
                    true // continue until end
                });
                warn!("After now leaks");
                out += &format!("\r\ntotal address:{}, bytes:{}\r\n", count, count_size);
                std::fs::write("leaks_log.txt", out.as_str().as_bytes()).ok();
            }
        });
    }
}
