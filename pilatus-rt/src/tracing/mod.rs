use std::{net::SocketAddr, path::PathBuf};

use pilatus::GenericConfig;
use tracing::info;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{prelude::*, util::TryInitError};

use self::logfile_writer::LogFileWriter;

mod logfile_writer;

pub(super) fn init(config: &GenericConfig) -> Result<WorkerGuard, TryInitError> {
    let default_filter_config = [
        "debug", //'fallback' level, if none of the following targets apply
        "hyper=info",
        "request=info",
        "async_zip=info",
        "tower_http=info",
        "mio_serial=info",
        "pilatus::image=info",
        "tungstenite::protocol=info",
    ]
    .join(",");

    let directory = config.instrument_relative(
        config
            .get("logdir")
            .unwrap_or_else(|_| PathBuf::from("logs")),
    );

    let num_files = config.get("log_files_number").unwrap_or(10);
    let (non_blocking, guard) = tracing_appender::non_blocking(LogFileWriter::new(
        tracing_appender::rolling::hourly(&directory, "pilatus-logs"),
        &directory,
        num_files,
    ));

    let def_clone = default_filter_config.clone();
    let registry = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_line_number(true)
                .compact()
                .with_filter(tracing_subscriber::EnvFilter::new(
                    config.get("tracing").unwrap_or(default_filter_config),
                )),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .compact()
                .with_ansi(false)
                .with_line_number(true)
                .with_filter(tracing_subscriber::EnvFilter::new(
                    config.get("tracing").unwrap_or(def_clone),
                )),
        );

    let result = if let Ok(socket) = config.get::<SocketAddr>("console-logger") {
        registry
            .with(
                console_subscriber::ConsoleLayer::builder()
                    .with_default_env()
                    .server_addr(socket)
                    .spawn(),
            )
            .try_init()
            .map(|_| guard)
    } else {
        registry.try_init().map(|_| guard)
    };
    info!(
        "Recording logs into: {:?}, keeping {num_files} files",
        directory.canonicalize()
    );

    result
}
