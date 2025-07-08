use std::{future::Future, io};

use anyhow::Result;
use minfac::{Resolvable, ServiceBuilder, ServiceCollection, WeakServiceProvider};
use minfac_utilities::{
    DepServiceProviderHandler, NodepServiceProviderHandler, ServiceProviderHandler,
};

mod minfac_utilities;

pub(crate) type HostedServiceResult = Result<()>;

pub trait ServiceBuilderExtensions {
    type Dependency: Send;

    fn register_hosted_service<TFut>(
        &mut self,
        name: &'static str,
        handler: fn(Self::Dependency) -> TFut,
    ) where
        TFut: Future<Output = HostedServiceResult> + Send + 'static;
}

impl<TDep> ServiceBuilderExtensions for ServiceBuilder<'_, TDep>
where
    TDep: Resolvable + 'static,
    TDep::ItemPreChecked: Send,
{
    type Dependency = TDep::ItemPreChecked;

    fn register_hosted_service<TFut>(
        &mut self,
        name: &'static str,
        handler: fn(TDep::ItemPreChecked) -> TFut,
    ) where
        TFut: Future<Output = HostedServiceResult> + Send + 'static,
    {
        let p =
            DepServiceProviderHandler::<HostedServiceResult, TDep, TFut>::new_boxed(name, handler);
        self.0.register_instance(HostedService::new(p))
    }
}

pub trait ServiceCollectionExtensions {
    fn register_hosted_service<TFut>(&mut self, name: &'static str, handler: fn() -> TFut)
    where
        TFut: Future<Output = HostedServiceResult> + Send + 'static;
}

impl ServiceCollectionExtensions for ServiceCollection {
    fn register_hosted_service<TFut>(&mut self, name: &'static str, handler: fn() -> TFut)
    where
        TFut: Future<Output = HostedServiceResult> + Send + 'static,
    {
        let p = NodepServiceProviderHandler::<HostedServiceResult, TFut>::new_boxed(name, handler);
        self.register_instance(HostedService::new(p))
    }
}

#[derive(Clone)]
pub struct HostedService(Box<dyn ServiceProviderHandler<HostedServiceResult>>);

impl HostedService {
    pub(crate) fn new(inner: Box<dyn ServiceProviderHandler<HostedServiceResult>>) -> Self {
        Self(inner)
    }

    pub fn get_name(&self) -> &str {
        self.0.get_name()
    }

    pub fn call(
        &self,
        provider: WeakServiceProvider,
    ) -> io::Result<tokio::task::JoinHandle<HostedServiceResult>> {
        self.0.call(provider)
    }
}
