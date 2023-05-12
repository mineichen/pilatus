use async_zip::base::read::stream::ZipFileReader;
use async_zip::base::read::WithEntry;
use futures::io::AsyncRead;
use futures::{future::BoxFuture, FutureExt};
use pilatus::{EntryItem, EntryReader};

use super::zip_to_io_error;

pub(super) struct ZipReaderWrapper<'a, T: AsyncRead + Unpin + Send + 'a>(ZipStates<'a, T>);

impl<'a, T: AsyncRead + Unpin + Send + 'a> ZipReaderWrapper<'a, T> {
    pub fn new(raw: T) -> Self {
        Self(ZipStates::Ready(ZipFileReader::new(raw)))
    }
}

#[allow(clippy::large_enum_variant)]
enum ZipStates<'a, T> {
    Ready(ZipFileReader<async_zip::base::read::stream::Ready<T>>),
    Reading(
        ZipFileReader<
            async_zip::base::read::stream::Reading<'a, futures::io::Take<T>, WithEntry<'a>>,
        >,
    ),
    Finished,
}

impl<'a, T: AsyncRead + Unpin + Send> EntryReader for ZipReaderWrapper<'a, T> {
    fn next(&mut self) -> BoxFuture<'_, Option<std::io::Result<EntryItem>>> {
        let mut current = ZipStates::Finished;
        std::mem::swap(&mut self.0, &mut current);
        async move {
            match current {
                ZipStates::Ready(x) => {
                    let next = x.next_with_entry().await;
                    match next {
                        Ok(Some(x)) => self.0 = ZipStates::Reading(x),
                        Ok(None) => return None,
                        Err(e) => return Some(Err(zip_to_io_error(e))),
                    }
                }
                ZipStates::Reading(y) => {
                    let next = y
                        .done()
                        .then(|e| async {
                            match e {
                                Ok(x) => Ok(x.next_with_entry().await?),
                                Err(e) => Err(e),
                            }
                        })
                        .await;
                    match next {
                        Ok(Some(x)) => {
                            self.0 = ZipStates::Reading(x);
                        }
                        Ok(None) => return None,
                        Err(e) => return Some(Err(zip_to_io_error(e))),
                    }
                }
                ZipStates::Finished => {
                    return None;
                }
            };
            let ZipStates::Reading(e) = &mut self.0 else { unreachable!();};
            let e = e.reader_mut();
            let filename = match e.entry().filename().clone().into_string() {
                Ok(x) => x,
                Err(e) => return Some(Err(zip_to_io_error(e))),
            };

            Some(Result::<_, std::io::Error>::Ok(EntryItem {
                filename,
                reader: Box::new(e),
            }))
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_zip::{Compression, ZipEntryBuilder};
    use futures::AsyncReadExt;

    #[tokio::test]
    async fn read_two_files() {
        let mut buf = Vec::new();
        let mut io = async_zip::base::write::ZipFileWriter::new(futures::io::Cursor::new(&mut buf));
        io.write_entry_whole(
            ZipEntryBuilder::new("test".into(), Compression::Deflate),
            b"data",
        )
        .await
        .unwrap();
        io.write_entry_whole(
            ZipEntryBuilder::new("test1".into(), Compression::Deflate),
            b"data1",
        )
        .await
        .unwrap();

        let mut x = ZipReaderWrapper::new(futures::io::Cursor::new(buf));
        {
            let mut first = x.next().await.unwrap().unwrap();
            let mut content = String::new();
            assert_eq!(first.filename, "test");
            first.reader.read_to_string(&mut content).await.unwrap();
            assert_eq!(content, "data");
        }
        {
            let mut second = x.next().await.unwrap().unwrap();
            let mut content = String::new();
            assert_eq!(second.filename, "test1");
            second.reader.read_to_string(&mut content).await.unwrap();
            assert_eq!(content, "data1");
        }
    }
}
