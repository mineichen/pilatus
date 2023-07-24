use crate::recipe::zip_to_io_error;
use async_zip::{base::write::ZipFileWriter, Compression, ZipEntryBuilder};
use futures::io::AsyncWrite;
use futures::{future::BoxFuture, AsyncReadExt, FutureExt};
use pilatus::{EntryWriter, PinReader};
use std::io;

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
            // TODO: This code would be better, but async_zip removed support in the 0.0.15 version. The following Error was raised
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
