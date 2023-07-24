use pilatus::TracingConfig;
use tracing::info;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{prelude::*, util::TryInitError};

use self::logfile_writer::LogFileWriter;

mod logfile_writer;

pub(super) fn init(config: &TracingConfig) -> Result<WorkerGuard, TryInitError> {
    let filter_config = config.log_string();

    let file = config.file().expect("Only works with file_logging enabled");

    let num_files = file.number_of_files;
    let (non_blocking, guard) = tracing_appender::non_blocking(LogFileWriter::new(
        tracing_appender::rolling::hourly(&file.path, "pilatus-logs"),
        &file.path,
        num_files,
    ));

    let def_clone = filter_config.clone();
    let registry = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_line_number(true)
                .compact()
                .with_filter(tracing_subscriber::EnvFilter::new(filter_config)),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .compact()
                .with_ansi(false)
                .with_line_number(true)
                .with_filter(tracing_subscriber::EnvFilter::new(def_clone)),
        );

    let result = if let Some(socket) = config.console().map(|x| &x.address) {
        registry
            .with(
                console_subscriber::ConsoleLayer::builder()
                    .with_default_env()
                    .server_addr(*socket)
                    .spawn(),
            )
            .try_init()
            .map(|_| guard)
    } else {
        registry.try_init().map(|_| guard)
    };
    info!(
        "Recording logs into: {:?}, keeping {num_files} files",
        file.path.canonicalize()
    );

    result
}
