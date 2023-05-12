use std::io;

use async_zip::{base::write::ZipFileWriter, Compression, ZipEntryBuilder};
use axum::body::StreamBody;
use bytes::Bytes;
use futures::io::AsyncWrite;
use futures::{future::BoxFuture, pin_mut, AsyncReadExt, FutureExt};
use minfac::ServiceCollection;
use pilatus::{EntryWriter, PinReader, RecipeExporter, RecipeId};
use pilatus_axum::{
    extract::{InjectRegistered, Path},
    http::StatusCode,
    AppendHeaders, IntoResponse, ServiceCollectionExtensions,
};

use super::zip_to_io_error;

pub(super) fn register_services(c: &mut ServiceCollection) {
    #[rustfmt::skip]
    c.register_web("recipe", |r| r
        .http("/:id/export",|m| m.get(export_recipe))
    );
}

pub struct ZipWriterWrapper<W: AsyncWrite + Unpin + Send + 'static>(ZipFileWriter<W>);

impl<W: AsyncWrite + Unpin + Send + 'static> ZipWriterWrapper<W> {
    pub fn new_boxed(raw: W) -> Box<Self> {
        Box::new(Self(ZipFileWriter::new(raw)))
    }
}

impl<W: AsyncWrite + Unpin + Send + 'static> EntryWriter for ZipWriterWrapper<W> {
    fn insert<'a>(
        &'a mut self,
        path: String,
        data: &'a mut dyn PinReader,
    ) -> BoxFuture<'a, io::Result<()>> {
        async move {
            let entry = ZipEntryBuilder::new(path.into(), Compression::Deflate).build();

            // start BadCode
            let mut materialized = Vec::with_capacity(entry.uncompressed_size() as _);
            data.read_to_end(&mut materialized).await?;
            self.0
                .write_entry_whole(entry, &materialized)
                .await
                .map_err(zip_to_io_error)?;

            // end BadCode
            /*
            // TODO: This code would be better, but async_zip removed support in the current version. The following Error was raised
            // ZipError::FeatureNotSupported("stream reading entries with data descriptors (planned to be reintroduced)")
            // So this should be supported again soon

            let mut writer = self
                .0
                .write_entry_stream(entry)
                .await
                .map_err(zip_to_io_error)?;
            copy(
                tokio_util::compat::TokioAsyncReadCompatExt::compat(data),
                &mut writer,
            )
            .await?;
            writer.close().await.map_err(zip_to_io_error)?;
            */
            Ok(())
        }
        .boxed()
    }

    fn close(self: Box<Self>) -> BoxFuture<'static, io::Result<()>> {
        async move {
            ZipFileWriter::close(self.0)
                .await
                .map(|_| ())
                .map_err(zip_to_io_error)
        }
        .boxed()
    }
}

async fn export_recipe(
    Path(recipe_id): Path<RecipeId>,
    InjectRegistered(service): InjectRegistered<RecipeExporter>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    Ok((
        AppendHeaders([(
            "Content-Disposition",
            format!("attachment; filename=\"{recipe_id}.pilatusrecipe\""),
        )]),
        StreamBody::new(async_stream::stream! {
            // Haven't found a non-duplex pipe...
            let (tx, mut rx) = tokio::io::duplex(2000);

            let fut = service.export(recipe_id, ZipWriterWrapper::new_boxed(tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(tx))).fuse();
            pin_mut!(fut);
            let mut buf = vec![0; 1500];

            loop {
                let reader = tokio::io::AsyncReadExt::read(&mut rx, &mut buf);
                pin_mut!(reader);

                let r = match futures::future::select(&mut fut, &mut reader).await {
                    futures::future::Either::Left((r, other)) => {
                        if let Err(e) = r {
                            yield Err(e);
                            break;
                        }
                        other.await
                    },
                    futures::future::Either::Right((r,_)) => r
                };
                match r {
                    Ok(num_bytes) => {
                        if num_bytes == 0 {
                            break;
                        }
                        yield Ok(Bytes::copy_from_slice(&buf[0..num_bytes]));
                    }
                    Err(e) => {
                        tracing::error!("Error streaming recipe-zip: {e}");
                        yield Err(anyhow::Error::from(e));
                        break;
                    }
                }

            }
        }),
    ))
}
