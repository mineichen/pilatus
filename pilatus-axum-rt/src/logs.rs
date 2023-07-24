use axum::response::IntoResponse;
use futures::{pin_mut, FutureExt, StreamExt};
use minfac::ServiceCollection;
use pilatus::{visit_directory_files, EntryWriter, TracingConfig};
use pilatus_axum::{
    extract::InjectRegistered, http::StatusCode, AppendHeaders, IoStreamBody,
    ServiceCollectionExtensions,
};
use std::path::PathBuf;
use tokio::fs;

use crate::zip_writer_wrapper::ZipWriterWrapper;

pub(super) fn register_services(c: &mut ServiceCollection) {
    #[rustfmt::skip]
    c.register_web("logs", |x| x
        .http("", |m| m.get(get_logs))
    );
}

async fn get_logs(
    InjectRegistered(config): InjectRegistered<TracingConfig>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let logs_dir = config
        .directory()
        .ok_or((StatusCode::BAD_REQUEST, "log directory is none!".to_owned()))?
        .to_owned();

    Ok((
        AppendHeaders([(
            "Content-Disposition",
            "attachment; filename=\"logs.zip\"".to_string(),
        )]),
        IoStreamBody::with_writer(move |w| {
            logfile_writer(logs_dir, ZipWriterWrapper::new_boxed(w)).fuse()
        }),
    ))
}

async fn logfile_writer(logs_dir: PathBuf, mut writer: Box<dyn EntryWriter>) -> anyhow::Result<()> {
    let files = visit_directory_files(logs_dir.clone());
    pin_mut!(files);

    while let Some(file) = files.next().await {
        let filename_full_path = file?.path();
        let entry_path = filename_full_path
            .strip_prefix(&logs_dir)?
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("invalid UTF-8"))?
            .to_owned();

        writer
            .insert(
                entry_path,
                &mut tokio_util::compat::TokioAsyncReadCompatExt::compat(
                    fs::File::open(filename_full_path).await?,
                ),
            )
            .await?;
    }

    writer.close().await?;
    Ok(())
}
