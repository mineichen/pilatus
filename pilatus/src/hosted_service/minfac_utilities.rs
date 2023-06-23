use std::io;

use futures::Future;
use minfac::{Resolvable, ServiceCollection, WeakServiceProvider};
use tokio::task::JoinHandle;

pub trait ServiceProviderHandler<T>: Send + Sync {
    fn clone_box(&self) -> Box<dyn ServiceProviderHandler<T>>;
    fn call(&self, provider: WeakServiceProvider) -> io::Result<JoinHandle<T>>;
    fn register_dummy_dependency(&self, col: &mut ServiceCollection);
    fn get_name(&self) -> &str;
}

impl<T> Clone for Box<dyn ServiceProviderHandler<T>> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

pub(crate) struct NodepServiceProviderHandler<T, TFut: Future<Output = T> + Send> {
    name: &'static str,
    handler: fn() -> TFut,
}

impl<T, TFut> NodepServiceProviderHandler<T, TFut>
where
    TFut: Future<Output = T> + Send,
{
    pub(crate) fn new_boxed(name: &'static str, handler: fn() -> TFut) -> Box<Self> {
        Box::new(NodepServiceProviderHandler::<T, TFut> { name, handler })
    }
}

impl<T: 'static + Send, TFut> ServiceProviderHandler<T> for NodepServiceProviderHandler<T, TFut>
where
    TFut: Future<Output = T> + Send + 'static,
{
    fn call(&self, _provider: WeakServiceProvider) -> io::Result<JoinHandle<T>> {
        let task = (self.handler)();
        #[cfg(tokio_unstable)]
        {
            tokio::task::Builder::new()
                .name(&format!("Hosted: {}", self.name))
                .spawn(task)
        }
        #[cfg(not(tokio_unstable))]
        Ok(tokio::task::spawn(task))
    }

    fn register_dummy_dependency(&self, _col: &mut ServiceCollection) {}

    fn clone_box(&self) -> Box<dyn ServiceProviderHandler<T>> {
        Box::new(NodepServiceProviderHandler::<T, TFut> {
            handler: self.handler,
            name: self.name,
        })
    }

    fn get_name(&self) -> &str {
        self.name
    }
}

pub(crate) struct DepServiceProviderHandler<T, TDep: Resolvable, TFut: Future<Output = T> + Send> {
    name: &'static str,
    handler: fn(TDep::ItemPreChecked) -> TFut,
}

impl<T, TDep, TFut> DepServiceProviderHandler<T, TDep, TFut>
where
    TDep: Resolvable,
    TFut: Future<Output = T> + Send,
{
    pub(crate) fn new_boxed(
        name: &'static str,
        handler: fn(TDep::ItemPreChecked) -> TFut,
    ) -> Box<Self> {
        Box::new(DepServiceProviderHandler::<T, TDep, TFut> { name, handler })
    }
}

impl<T: 'static + Send, TDep, TFut> ServiceProviderHandler<T>
    for DepServiceProviderHandler<T, TDep, TFut>
where
    TDep: Resolvable + Send + 'static,
    TDep::ItemPreChecked: Send,
    TFut: Future<Output = T> + Send + 'static,
{
    fn call(&self, provider: WeakServiceProvider) -> io::Result<JoinHandle<T>> {
        let task = (self.handler)(provider.resolve_unchecked::<TDep>());
        #[cfg(tokio_unstable)]
        {
            tokio::task::Builder::new()
                .name(&format!("Hosted: {}", self.name))
                .spawn(task)
        }
        #[cfg(not(tokio_unstable))]
        Ok(tokio::task::spawn(task))
    }

    fn register_dummy_dependency(&self, col: &mut ServiceCollection) {
        col.with::<TDep>().register(|_| ());
    }

    fn clone_box(&self) -> Box<dyn ServiceProviderHandler<T>> {
        Box::new(DepServiceProviderHandler::<T, TDep, TFut> {
            handler: self.handler,
            name: self.name,
        })
    }

    fn get_name(&self) -> &str {
        self.name
    }
}
