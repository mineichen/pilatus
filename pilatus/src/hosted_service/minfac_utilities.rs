use std::io;
use std::marker::PhantomData;

use minfac::{Resolvable, WeakServiceProvider};
use tokio::task::JoinHandle;

use super::{HostedServiceFn, HostedServiceResult};
use crate::ReadySignal;

pub trait ServiceProviderHandler<T>: Send + Sync {
    fn clone_box(&self) -> Box<dyn ServiceProviderHandler<T>>;
    fn call(&self, provider: WeakServiceProvider, ready: ReadySignal) -> io::Result<JoinHandle<T>>;
    fn get_name(&self) -> &'static str;
}

impl<T> Clone for Box<dyn ServiceProviderHandler<T>> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

pub(crate) struct DepServiceProviderHandler<TDep, TFn, TMarker> {
    name: &'static str,
    handler: TFn,
    phantom: PhantomData<fn() -> (TDep, TMarker)>,
}

impl<TDep, TFn, TMarker> DepServiceProviderHandler<TDep, TFn, TMarker>
where
    TDep: Resolvable,
    TFn: HostedServiceFn<TDep::ItemPreChecked, TMarker>,
{
    pub(crate) fn new_boxed(name: &'static str, handler: TFn) -> Box<Self> {
        Box::new(DepServiceProviderHandler {
            name,
            handler,
            phantom: PhantomData,
        })
    }
}

impl<TDep, TFn, TMarker> ServiceProviderHandler<HostedServiceResult>
    for DepServiceProviderHandler<TDep, TFn, TMarker>
where
    TDep: Resolvable + 'static,
    TFn: HostedServiceFn<TDep::ItemPreChecked, TMarker>,
    TMarker: 'static,
{
    fn call(
        &self,
        provider: WeakServiceProvider,
        ready: ReadySignal,
    ) -> io::Result<JoinHandle<HostedServiceResult>> {
        let task = self
            .handler
            .create(ready, provider.resolve_unchecked::<TDep>());
        #[cfg(tokio_unstable)]
        {
            tokio::task::Builder::new()
                .name(&format!("Hosted: {}", self.name))
                .spawn(task)
        }
        #[cfg(not(tokio_unstable))]
        Ok(tokio::task::spawn(task))
    }

    fn clone_box(&self) -> Box<dyn ServiceProviderHandler<HostedServiceResult>> {
        Box::new(DepServiceProviderHandler::<TDep, TFn, TMarker> {
            handler: self.handler,
            name: self.name,
            phantom: PhantomData,
        })
    }

    fn get_name(&self) -> &'static str {
        self.name
    }
}
