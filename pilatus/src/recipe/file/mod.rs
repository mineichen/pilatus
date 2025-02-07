//! Running devices are responsible for all file accesses in their folder
//!
//! If a Recipe is not running, the RecipeService is allowed to modify files (e.g. import/export)

use std::{
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::Arc,
};

pub use device::*;
use futures::{future::BoxFuture, stream::BoxStream, FutureExt};
use tracing::trace;

use crate::{
    device::DeviceId, RelativeDirectoryPath, RelativeDirectoryPathBuf, RelativeFilePath,
    TransactionError,
};

mod device;

type InnerService = Box<dyn FileServiceTrait + Send + Sync>;
type InnerFactory = Arc<dyn Fn(DeviceId) -> InnerService + Send + Sync>;

#[derive(Clone)]
pub struct FileServiceBuilder {
    pub inner_factory: InnerFactory,
}

impl FileServiceBuilder {
    pub fn with_validator<T: 'static, TValidator: Validator<State = T> + 'static>(
        self,
        validator: TValidator,
    ) -> TypedFileServiceBuilder<T> {
        TypedFileServiceBuilder::<T>::from(self).with_validator(validator)
    }
    pub fn build(self, device_id: DeviceId) -> FileService<()> {
        TypedFileServiceBuilder::<()>::from(self).build(device_id)
    }
}

impl<T> From<FileServiceBuilder> for TypedFileServiceBuilder<T> {
    fn from(b: FileServiceBuilder) -> Self {
        Self {
            inner_factory: b.inner_factory,
            validators: Vec::new(),
        }
    }
}

pub struct TypedFileServiceBuilder<T> {
    inner_factory: InnerFactory,
    pub validators: Vec<Box<dyn Validator<State = T>>>,
}

impl<T: 'static> TypedFileServiceBuilder<T> {
    pub fn with_validator<TValidator: Validator<State = T> + 'static>(
        mut self,
        validator: TValidator,
    ) -> Self {
        self.validators.push(Box::new(validator));
        self
    }

    pub fn build(self, device_id: DeviceId) -> FileService<T> {
        FileService {
            inner: (self.inner_factory)(device_id),
            validators: Arc::new(self.validators),
        }
    }
}

pub trait Validator: Send + Sync {
    type State;

    fn is_responsible(&self, path: &RelativeFilePath) -> bool;
    fn validate<'a>(
        &self,
        data: &'a [u8],
        ctx: &'a mut Self::State,
    ) -> BoxFuture<'a, Result<(), anyhow::Error>>;
}

pub struct FileService<T = ()> {
    inner: Box<dyn FileServiceTrait + Send + Sync>,
    validators: Arc<Vec<Box<dyn Validator<State = T>>>>,
}

impl<T> Deref for FileService<T> {
    type Target = dyn FileServiceTrait + Send + Sync;

    fn deref(&self) -> &Self::Target {
        self.inner.as_ref()
    }
}
impl<T> DerefMut for FileService<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.as_mut()
    }
}

#[async_trait::async_trait]
pub trait FileServiceTrait {
    async fn has_file(&self, filename: &RelativeFilePath) -> Result<bool, TransactionError>;
    async fn list_recursive(&self, path: &RelativeDirectoryPath) -> std::io::Result<Vec<PathBuf>>;
    // If the parent doesn't exist, it will be created recursively
    async fn add_file_unchecked(
        &mut self,
        file_path: &RelativeFilePath,
        data: &[u8],
    ) -> Result<(), anyhow::Error>;
    async fn remove_file(&self, filename: &RelativeFilePath) -> Result<(), TransactionError>;
    async fn get_file(&self, filename: &RelativeFilePath) -> Result<Vec<u8>, TransactionError>;
    async fn list_files(
        &self,
        path: &RelativeDirectoryPath,
    ) -> Result<Vec<RelativeFilePath>, TransactionError>;
    async fn get_or_create_directory(
        &self,
        dir_path: &RelativeDirectoryPath,
    ) -> anyhow::Result<PathBuf>;
    fn stream_files(
        &self,
        path: &RelativeDirectoryPath,
    ) -> BoxStream<'static, Result<RelativeFilePath, TransactionError>>;
    fn stream_directories(
        &self,
        path: &RelativeDirectoryPath,
    ) -> BoxStream<'static, Result<RelativeDirectoryPathBuf, TransactionError>>;
    fn get_filepath(&self, file_path: &RelativeFilePath) -> PathBuf;
    fn get_directory_path(&self, file_path: &RelativeDirectoryPath) -> PathBuf;
    fn get_root(&self) -> &Path;
}

pub trait FileServiceExt {
    fn has_validator_for(&self, path: &RelativeFilePath) -> bool;
    fn add_file_validated<'a>(
        &'a mut self,
        file_path: &'a RelativeFilePath,
        data: &'a [u8],
    ) -> BoxFuture<'a, Result<(), anyhow::Error>>;
}

impl<T: AsMut<FileService<T>> + AsRef<FileService<T>> + Send + Sync> FileServiceExt for T {
    fn has_validator_for(&self, path: &RelativeFilePath) -> bool {
        self.as_ref()
            .validators
            .iter()
            .any(|p| p.is_responsible(path))
    }
    fn add_file_validated<'a>(
        &'a mut self,
        file_path: &'a RelativeFilePath,
        data: &'a [u8],
    ) -> BoxFuture<'a, Result<(), anyhow::Error>> {
        trace!(filename = ?file_path, "Create file validated");
        async move {
            let validators = self.as_mut().validators.clone();

            validators
                .iter()
                .find(|x| x.is_responsible(file_path))
                .ok_or_else(|| anyhow::anyhow!("Coultn't find responsible validator"))?
                .validate(data, self)
                .await?;
            self.as_mut().add_file_unchecked(file_path, data).await
        }
        .boxed()
    }
}
