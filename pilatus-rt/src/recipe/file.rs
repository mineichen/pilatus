use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use futures::{
    stream::{self, BoxStream},
    Stream, StreamExt, TryStreamExt,
};
use minfac::{Registered, ServiceCollection};
use pilatus::{
    device::DeviceId, FileService, FileServiceBuilder, FileServiceTrait, RelativeDirPath,
    RelativeFilePath, TransactionError,
};
use tokio::{
    fs::{self, File},
    io::{AsyncReadExt, AsyncWriteExt},
};
use tracing::trace;

use super::RecipeServiceImpl;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<Registered<Arc<RecipeServiceImpl>>>()
        .register(|r| r.build_file_service());
}

impl RecipeServiceImpl {
    pub(super) fn build_file_service(&self) -> FileServiceBuilder {
        TokioFileService::builder(self.get_recipe_dir_path())
    }

    pub(super) fn build_device_file_service(&self, device_id: DeviceId) -> FileService<()> {
        self.build_file_service().build(device_id)
    }
}

#[async_trait::async_trait]
impl FileServiceTrait for TokioFileService {
    async fn has_file(&self, filename: &RelativeFilePath) -> Result<bool, TransactionError> {
        let s = self.get_filepath(filename);
        Ok(fs::metadata(s).await.is_ok())
    }

    async fn list_recursive(&self) -> std::io::Result<Vec<PathBuf>> {
        pilatus::visit_directory_files(self.get_device_dir())
            .take_while(|f| {
                std::future::ready(if let Err(e) = f {
                    e.kind() != std::io::ErrorKind::NotFound
                } else {
                    true
                })
            })
            .map(|f| f.map(|f| f.path()))
            .try_collect()
            .await
    }
    async fn add_file_unchecked(
        &mut self,
        file_path: &RelativeFilePath,
        data: &[u8],
    ) -> Result<(), anyhow::Error> {
        trace!(filename = ?file_path, "Create file unchecked");
        let p = self.get_filepath(file_path);
        fs::create_dir_all(
            p.parent()
                .expect("RelativeFilePath always have a parent folder"),
        )
        .await?;

        fs::write(&p, data).await?;
        Ok(())
    }

    async fn remove_file(&self, filename: &RelativeFilePath) -> Result<(), TransactionError> {
        let p = self.get_filepath(filename);

        //remove file from folder
        fs::remove_file(&p).await.map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => TransactionError::UnknownFilePath(p.clone()),
            _ => TransactionError::FileSystemError(e),
        })?;

        //remove parent folder if it is now empty
        if let Some(p) = p.parent() {
            let is_dir_empty = fs::read_dir(p).await?.next_entry().await?.is_none();

            if is_dir_empty {
                fs::remove_dir(p).await?
            }
        }

        Ok(())
    }

    async fn get_file(&self, filename: &RelativeFilePath) -> Result<Vec<u8>, TransactionError> {
        let p = self.get_filepath(filename);

        if !p.exists() {
            Err(TransactionError::UnknownFilePath(p))
        } else {
            let f = fs::File::open(p).await?;

            let mut buf = Vec::new();
            tokio::io::BufReader::new(f).read_to_end(&mut buf).await?;

            Ok(buf)
        }
    }

    fn stream_files(
        &self,
        path: &RelativeDirPath,
    ) -> BoxStream<'static, Result<RelativeFilePath, TransactionError>> {
        self.stream_files_internal(path).boxed()
    }

    async fn list_files(
        &self,
        path: &RelativeDirPath,
    ) -> Result<Vec<RelativeFilePath>, TransactionError> {
        self.stream_files_internal(path)
            .try_collect::<Vec<RelativeFilePath>>()
            .await
    }

    // RelativeFilePath is expected to be relative to the device-folder
    // The returned PathBuf can be used to e.g. open a file with std::fs::File::open().
    fn get_filepath(&self, file_path: &RelativeFilePath) -> PathBuf {
        self.get_device_dir().join(file_path.get_path())
    }
}

pub struct TokioFileService {
    root: PathBuf,
}
impl TokioFileService {
    pub fn builder(root: impl Into<PathBuf>) -> FileServiceBuilder {
        let root = root.into();
        FileServiceBuilder {
            inner_factory: Arc::new(move |device_id| {
                Box::new(TokioFileService {
                    root: root.join(device_id.to_string()),
                })
            }),
        }
    }

    fn stream_files_internal(
        &self,
        path: &RelativeDirPath,
    ) -> impl Stream<Item = Result<RelativeFilePath, TransactionError>> + 'static {
        let device_dir: Arc<Path> = self.get_device_dir().to_owned().into();
        let dir_path: Arc<Path> = device_dir.join(path.as_path()).into();

        stream::once(fs::read_dir(dir_path.clone())).flat_map(move |x| {
            let dir = match x {
                Ok(x) => x,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return stream::empty().boxed()
                }
                Err(e) => {
                    return stream::iter([Err(TransactionError::from_io_producer(&dir_path)(e))])
                        .boxed()
                }
            };
            let dir_path = dir_path.clone();
            let device_dir = device_dir.clone();

            tokio_stream::wrappers::ReadDirStream::new(dir)
                .filter_map(move |entry| {
                    let dir_path = dir_path.clone();
                    let device_dir = device_dir.clone();
                    async move {
                        let io_err_converter = TransactionError::from_io_producer(&dir_path);
                        let entry = match entry {
                            Ok(x) => x,
                            Err(e) => return Some(Err((io_err_converter)(e))),
                        };
                        let file_type = match entry.file_type().await {
                            Ok(x) => x,
                            Err(e) => return Some(Err((io_err_converter)(e))),
                        };
                        if !file_type.is_file() {
                            return None;
                        }

                        let p = entry.path();
                        let p = p
                            .strip_prefix(device_dir)
                            .expect("ReadDirStream returns relative entries");

                        Some(RelativeFilePath::new(p).map_err(|e| anyhow::Error::from(e).into()))
                    }
                })
                .boxed()
        })
        //            .map_err(TransactionError::from_io_producer(&dir_path))?;
    }

    fn get_device_dir(&self) -> &PathBuf {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use futures::{future::BoxFuture, FutureExt};
    use pilatus::{device::DeviceId, FileServiceExt, Validator};

    use super::*;
    use pilatus::{FileService, RelativeDirPath, RelativeFilePath};

    #[tokio::test]
    async fn add_valid() -> anyhow::Result<()> {
        add_file_validated_works("HelloWorld").await
    }

    #[tokio::test]
    async fn add_invalid() {
        add_file_validated_works("Invalid")
            .await
            .expect_err("Shouldn't work");
    }

    async fn add_file_validated_works(file_content: &str) -> anyhow::Result<()> {
        struct Ctx {
            answer: i32,
            file_service: FileService<Ctx>,
        }

        impl AsRef<FileService<Ctx>> for Ctx {
            fn as_ref(&self) -> &FileService<Ctx> {
                &self.file_service
            }
        }

        impl AsMut<FileService<Ctx>> for Ctx {
            fn as_mut(&mut self) -> &mut FileService<Ctx> {
                &mut self.file_service
            }
        }

        struct ContainsHello;

        impl Validator for ContainsHello {
            type State = Ctx;
            fn is_responsible(&self, _: &RelativeFilePath) -> bool {
                true
            }

            fn validate<'a>(
                &self,
                data: &'a [u8],
                ctx: &'a mut Ctx,
            ) -> BoxFuture<'a, Result<(), anyhow::Error>> {
                async {
                    if std::str::from_utf8(data)?.contains("Hello") && ctx.answer == 0 {
                        Ok(())
                    } else {
                        Err(anyhow::anyhow!("Must contain 'Hello'"))
                    }
                }
                .boxed()
            }
        }

        let dir = tempfile::tempdir()?;
        let device_id = DeviceId::new_v4();
        let mut ctx = Ctx {
            answer: 0,
            file_service: TokioFileService::builder(dir.path())
                .with_validator(ContainsHello)
                .build(device_id),
        };
        ctx.add_file_validated(
            &RelativeFilePath::new("test.jpg").unwrap(),
            file_content.as_bytes(),
        )
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn list_files() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let device_id = DeviceId::new_v4();
        let mut svc = TokioFileService::builder(dir.path()).build(device_id);

        for (dir, file) in [
            (
                RelativeDirPath::new("sub")?,
                RelativeFilePath::new("sub/image.jpg")?,
            ),
            (RelativeDirPath::root(), RelativeFilePath::new("image.jpg")?),
        ] {
            assert!(
                svc.list_files(&dir).await?.is_empty(),
                "Works without a device-directory"
            );
            svc.add_file_unchecked(&file, b"Text").await?;
            assert_eq!(vec![file], svc.list_files(&dir).await?);
        }
        Ok(())
    }
}
