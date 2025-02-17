use std::{
    fs::FileType,
    path::{Path, PathBuf},
    sync::Arc,
};

use futures::{
    stream::{self, BoxStream},
    Stream, StreamExt, TryStreamExt,
};
use minfac::{Registered, ServiceCollection};
use pilatus::{
    FileServiceBuilder, FileServiceTrait, RelativeDirectoryPath, RelativeDirectoryPathBuf,
    RelativeFilePath, TransactionError,
};
use tokio::{fs, io::AsyncReadExt};
use tracing::trace;

use super::RecipeServiceFassade;

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.with::<Registered<Arc<RecipeServiceFassade>>>()
        .register(|r| r.build_file_service());
}

#[async_trait::async_trait]
impl FileServiceTrait for TokioFileService {
    async fn has_file(&self, filename: &RelativeFilePath) -> Result<bool, TransactionError> {
        let s = self.get_filepath(filename);
        Ok(fs::metadata(s).await.is_ok())
    }

    async fn list_recursive(&self, root: &RelativeDirectoryPath) -> std::io::Result<Vec<PathBuf>> {
        pilatus::visit_directory_files(&self.root.join(root))
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
        &self,
        file_path: &RelativeFilePath,
        data: &[u8],
    ) -> Result<(), anyhow::Error> {
        trace!(filename = ?file_path, "Create file unchecked");
        self.get_or_create_directory(file_path.relative_dir())
            .await?;
        fs::write(self.get_filepath(file_path), data).await?;
        Ok(())
    }

    async fn get_or_create_directory(
        &self,
        path: &RelativeDirectoryPath,
    ) -> anyhow::Result<PathBuf> {
        let p = self.get_directory_path(path);
        fs::create_dir_all(&p).await?;
        Ok(p)
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

    fn stream_files_recursive(
        &self,
        path: &RelativeDirectoryPath,
    ) -> BoxStream<'static, Result<RelativeFilePath, TransactionError>> {
        let path = self.root.join(path);
        let root_clone = self.root.clone();
        pilatus::visit_directory_files(&path)
            .take_while(|f| {
                std::future::ready(if let Err(e) = f {
                    e.kind() != std::io::ErrorKind::NotFound
                } else {
                    true
                })
            })
            .map_err(TransactionError::other)
            .try_filter_map(move |relative_to_pilatus_entry| {
                let r = RelativeFilePath::new(
                    relative_to_pilatus_entry
                        .path()
                        .strip_prefix(&root_clone)
                        .expect("Iteration was done in root"),
                )
                .map_err(|e| TransactionError::other(e));
                async move { Ok(Some(r?)) }
            })
            .boxed()
    }
    fn stream_files(
        &self,
        path: &RelativeDirectoryPath,
    ) -> BoxStream<'static, Result<RelativeFilePath, TransactionError>> {
        self.stream_files_internal(path, |file_type, path| {
            if !file_type.is_file() {
                return None;
            }
            Some(RelativeFilePath::new(path).map_err(|e| anyhow::Error::from(e).into()))
        })
        .boxed()
    }

    fn stream_directories(
        &self,
        path: &RelativeDirectoryPath,
    ) -> BoxStream<'static, Result<RelativeDirectoryPathBuf, TransactionError>> {
        self.stream_files_internal(path, |file_type, path| {
            if !file_type.is_dir() {
                return None;
            }
            Some(RelativeDirectoryPathBuf::new(path).map_err(|e| anyhow::Error::from(e).into()))
        })
        .boxed()
    }

    async fn list_files(
        &self,
        path: &RelativeDirectoryPath,
    ) -> Result<Vec<RelativeFilePath>, TransactionError> {
        self.stream_files_internal(path, |file_type, path| {
            if !file_type.is_file() {
                return None;
            }
            Some(RelativeFilePath::new(path).map_err(|e| anyhow::Error::from(e).into()))
        })
        .try_collect::<Vec<RelativeFilePath>>()
        .await
    }

    // RelativeFilePath is expected to be relative to the device-folder
    // The returned PathBuf can be used to e.g. open a file with std::fs::File::open().
    fn get_filepath(&self, file_path: &RelativeFilePath) -> PathBuf {
        self.root.join(file_path.get_path())
    }
    fn get_directory_path(&self, dir_path: &RelativeDirectoryPath) -> PathBuf {
        self.root.join(dir_path)
    }

    fn get_root(&self) -> &Path {
        &self.root
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

    fn stream_files_internal<T: Send + 'static>(
        &self,
        path: &RelativeDirectoryPath,
        filter_map: fn(FileType, &Path) -> Option<Result<T, TransactionError>>,
    ) -> impl Stream<Item = Result<T, TransactionError>> + 'static {
        let device_dir: Arc<Path> = self.root.to_owned().into();
        let dir_path: Arc<Path> = device_dir.join(path).into();

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
                        let p = entry.path();
                        let p = p
                            .strip_prefix(device_dir)
                            .expect("ReadDirStream returns relative entries");
                        filter_map(file_type, p)
                    }
                })
                .boxed()
        })
        //            .map_err(TransactionError::from_io_producer(&dir_path))?;
    }
}

#[cfg(test)]
mod tests {
    use futures::{future::BoxFuture, FutureExt};
    use pilatus::{device::DeviceId, FileServiceExt, Validator};

    use super::*;
    use pilatus::{FileService, RelativeDirectoryPathBuf, RelativeFilePath};

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
    async fn stream_files_recursive_works() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let device_id = DeviceId::new_v4();
        let svc = TokioFileService::builder(dir.path()).build(device_id);

        svc.add_file_unchecked(&RelativeFilePath::new("foo/bar/baz.jpg")?, &vec![0u8])
            .await?;
        svc.add_file_unchecked(&RelativeFilePath::new("foo/baz/bar.png")?, &vec![0u8])
            .await?;
        svc.add_file_unchecked(&RelativeFilePath::new("foo/baz/bar.jpg")?, &vec![0u8])
            .await?;

        let mut baz = svc
            .stream_files_recursive(&RelativeDirectoryPath::new("foo/baz")?)
            .map_ok(|x| x.as_os_str().to_string_lossy().to_string())
            .try_collect::<Vec<_>>()
            .await?;

        baz.sort_unstable();
        assert_eq!(baz, vec!["foo/baz/bar.jpg", "foo/baz/bar.png"]);

        Ok(())
    }

    #[tokio::test]
    async fn list_files() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let device_id = DeviceId::new_v4();
        let svc = TokioFileService::builder(dir.path()).build(device_id);

        for (dir, file) in [
            (
                RelativeDirectoryPathBuf::new("sub")?,
                RelativeFilePath::new("sub/image.jpg")?,
            ),
            (
                RelativeDirectoryPathBuf::root(),
                RelativeFilePath::new("image.jpg")?,
            ),
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
