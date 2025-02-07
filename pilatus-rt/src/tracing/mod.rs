use std::sync::{Arc, OnceLock};

use minfac::{Registered, ServiceCollection, ServiceProvider};
use pilatus::{GenericConfig, TracingConfig, TracingTopic};
use tracing::{debug, info, warn};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{prelude::*, reload, util::TryInitError, EnvFilter};

use self::logfile_writer::LogFileWriter;

mod logfile_writer;

/// Initializes tracing during the ServiceProvider::register_services phase
/// Init must be called afterwards to allow plugins to affect the logging
pub(super) fn pre_init(config: &GenericConfig, services: &mut ServiceCollection) -> bool {
    let tracing_config = TracingConfig::from((config, []));

    services
        .with::<Registered<Arc<TracingState>>>()
        .register::<TracingConfig>(|c| {
            c.config
                .get()
                .expect("tracing::init must be called to setup the final logging")
                .clone()
        });
    let (result, state) = init_tracing(&tracing_config);
    services.register_instance(Arc::new(state));
    result.is_ok()
}

pub struct TracingState {
    _handle: WorkerGuard,
    // Used to update the TracingLevels when tracing is running already
    updater: Box<dyn Fn(&TracingConfig) + Send + Sync>,
    config: OnceLock<TracingConfig>,
}

pub(super) fn init(
    p: &ServiceProvider,
    pre_init_success: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config: GenericConfig = p.get().ok_or("Expects to have GenericConfig")?;
    let tracing_state: Arc<TracingState> = p
        .get()
        .ok_or("Expects to have TracingState (have you called pre_init?)")?;

    let tracing_config = TracingConfig::from((&config, p.get_all::<TracingTopic>()));
    if pre_init_success {
        debug!("Use trace-filter: {}", tracing_config.log_string());
        (tracing_state.updater)(&tracing_config);
    } else {
        warn!("PreInit tracing failed. It was probably initialized already.");
    }

    tracing_state
        .config
        .set(tracing_config)
        .map_err(|_| "tracing::init should only be called once")?;
    Ok(())
}

fn init_tracing(config: &TracingConfig) -> (Result<(), TryInitError>, TracingState) {
    let filter_config = config.log_string();

    let file = config
        .file()
        .expect("Optional file logging can be configured, but is not yet implemented");
    let terminal = config
        .terminal()
        .expect("Optional terminal logging can be configured, but is not yet implemented");

    let num_files = file.number_of_files;
    let (non_blocking, guard) = tracing_appender::non_blocking(LogFileWriter::new(
        tracing_appender::rolling::hourly(&file.path, "pilatus-logs"),
        &file.path,
        num_files,
    ));

    let (term_level_filter, term_level_updater) =
        reload::Layer::new(EnvFilter::new(&filter_config));
    let (file_level_filter, file_level_updater) =
        reload::Layer::new(EnvFilter::new(&filter_config));

    let updater = Box::new(move |tracing_config: &TracingConfig| {
        file_level_updater
            .modify(|f| *f = EnvFilter::new(tracing_config.log_string()))
            .expect("Couldn't update file-log-level");
        term_level_updater
            .modify(|f| *f = EnvFilter::new(tracing_config.log_string()))
            .expect("Couldn't update term-log-level");
    });

    let terminal_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_line_number(true)
        .with_ansi(terminal.ansi)
        .with_filter(term_level_filter);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .compact()
        .with_line_number(true)
        .with_ansi(false)
        .with_filter(file_level_filter);
    let registry = tracing_subscriber::registry()
        .with(terminal_layer)
        .with(file_layer);

    let result = if let Some(socket) = config.console().map(|x| &x.address) {
        let r = registry
            .with(
                console_subscriber::ConsoleLayer::builder()
                    .with_default_env()
                    .server_addr(*socket)
                    .spawn(),
            )
            .try_init();
        info!("Setup tokio-console on socket: {socket:?}",);

        r
    } else {
        let r = registry.try_init();
        info!("tokio-console is disabled");
        r
    };
    info!(
        "Recording logs into: {:?}, keeping {num_files} files",
        file.path.canonicalize(),
    );

    (
        result,
        TracingState {
            config: OnceLock::<TracingConfig>::new(),
            _handle: guard,
            updater,
        },
    )
}
