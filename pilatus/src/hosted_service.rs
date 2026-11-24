use std::{future::Future, io, marker::PhantomData};

use anyhow::Result;
use minfac::{Resolvable, ServiceBuilder, ServiceCollection, WeakServiceProvider};
use minfac_utilities::{DepServiceProviderHandler, ServiceProviderHandler};

use crate::ReadySignal;

mod minfac_utilities;

pub(crate) type HostedServiceResult = Result<()>;

/// Function, which creates the future of a hosted service from its dependencies.
///
/// It is implemented for `fn(TDep)` and `fn(ReadySignal, TDep)` (`fn()` and `fn(ReadySignal)` without dependencies).
/// Services without [`ReadySignal`] are ready as soon as they are started.
/// `TMarker` distinguishes these signatures.
pub trait HostedServiceFn<TDep, TMarker>: Copy + Send + Sync + 'static {
    type Future: Future<Output = HostedServiceResult> + Send + 'static;

    fn create(self, ready: ReadySignal, dep: TDep) -> Self::Future;
}

impl<TFn, TDep, TFut> HostedServiceFn<TDep, (TDep,)> for TFn
where
    TFn: Fn(TDep) -> TFut + Copy + Send + Sync + 'static,
    TFut: Future<Output = HostedServiceResult> + Send + 'static,
{
    type Future = TFut;

    fn create(self, ready: ReadySignal, dep: TDep) -> TFut {
        ready.signal();
        self(dep)
    }
}

impl<TFn, TDep, TFut> HostedServiceFn<TDep, (ReadySignal, TDep)> for TFn
where
    TFn: Fn(ReadySignal, TDep) -> TFut + Copy + Send + Sync + 'static,
    TFut: Future<Output = HostedServiceResult> + Send + 'static,
{
    type Future = TFut;

    fn create(self, ready: ReadySignal, dep: TDep) -> TFut {
        self(ready, dep)
    }
}

impl<TFn, TFut> HostedServiceFn<(), ()> for TFn
where
    TFn: Fn() -> TFut + Copy + Send + Sync + 'static,
    TFut: Future<Output = HostedServiceResult> + Send + 'static,
{
    type Future = TFut;

    fn create(self, ready: ReadySignal, (): ()) -> TFut {
        ready.signal();
        self()
    }
}

impl<TFn, TFut> HostedServiceFn<(), (ReadySignal,)> for TFn
where
    TFn: Fn(ReadySignal) -> TFut + Copy + Send + Sync + 'static,
    TFut: Future<Output = HostedServiceResult> + Send + 'static,
{
    type Future = TFut;

    fn create(self, ready: ReadySignal, (): ()) -> TFut {
        self(ready)
    }
}

/// Never resolved. Registered with the dependencies of a hosted service, so `ServiceCollection::build()` fails if one is missing.
/// The error names the hosted service function.
struct HostedServiceDependencies<TFn>(PhantomData<TFn>);

pub trait ServiceBuilderExtensions {
    type Dependency: Send;

    /// Registers a service, which runs from startup until it returns.
    ///
    /// If `handler` takes a [`ReadySignal`] as first parameter, the service tells the runtime once it is ready.
    /// It keeps running after calling [`ReadySignal::signal`], even if other services aren't ready yet.
    /// Services without [`ReadySignal`] are ready as soon as they are started.
    /// The runtime of `pilatus-rt` polls the future passed to `run`, `run_until_finished` or `run_until` once all services are ready.
    ///
    /// If the service stops without signaling (e.g. because it failed), it isn't waited for anymore.
    ///
    /// ```
    /// use minfac::{Registered, ServiceCollection};
    /// use pilatus::{prelude::*, ReadySignal, SystemShutdown};
    ///
    /// extern "C" fn register(c: &mut ServiceCollection) {
    ///     c.with::<Registered<SystemShutdown>>()
    ///         .register_hosted_service("Example", example_service);
    /// }
    ///
    /// async fn example_service(ready: ReadySignal, shutdown: SystemShutdown) -> anyhow::Result<()> {
    ///     // Setup (e.g. bind a TcpListener) and fail with `?` if it doesn't work
    ///     ready.signal();
    ///     shutdown.await;
    ///     Ok(())
    /// }
    /// ```
    fn register_hosted_service<TFn, TMarker>(&mut self, name: &'static str, handler: TFn)
    where
        TFn: HostedServiceFn<Self::Dependency, TMarker>,
        TMarker: 'static;
}

impl<TDep> ServiceBuilderExtensions for ServiceBuilder<'_, TDep>
where
    TDep: Resolvable + 'static,
    TDep::ItemPreChecked: Send,
{
    type Dependency = TDep::ItemPreChecked;

    fn register_hosted_service<TFn, TMarker>(&mut self, name: &'static str, handler: TFn)
    where
        TFn: HostedServiceFn<TDep::ItemPreChecked, TMarker>,
        TMarker: 'static,
    {
        self.register(|_| HostedServiceDependencies::<TFn>(PhantomData));
        let p = DepServiceProviderHandler::<TDep, _, TMarker>::new_boxed(name, handler);
        self.0.register_instance(HostedService::new(p))
    }
}

pub trait ServiceCollectionExtensions {
    /// Registers a service without dependencies. See [`ServiceBuilderExtensions::register_hosted_service`]
    fn register_hosted_service<TFn, TMarker>(&mut self, name: &'static str, handler: TFn)
    where
        TFn: HostedServiceFn<(), TMarker>,
        TMarker: 'static;
}

impl ServiceCollectionExtensions for ServiceCollection {
    fn register_hosted_service<TFn, TMarker>(&mut self, name: &'static str, handler: TFn)
    where
        TFn: HostedServiceFn<(), TMarker>,
        TMarker: 'static,
    {
        let p = DepServiceProviderHandler::<(), _, TMarker>::new_boxed(name, handler);
        self.register_instance(HostedService::new(p))
    }
}

#[derive(Clone)]
pub struct HostedService(Box<dyn ServiceProviderHandler<HostedServiceResult>>);

impl HostedService {
    pub(crate) fn new(inner: Box<dyn ServiceProviderHandler<HostedServiceResult>>) -> Self {
        Self(inner)
    }

    pub fn get_name(&self) -> &'static str {
        self.0.get_name()
    }

    /// Spawns the service, which triggers `ready` once it is ready.
    ///
    /// Services registered without [`ReadySignal`] trigger it immediately.
    /// If the service stops before it is ready (or can't be spawned), `ready` is dropped without signaling.
    pub fn call(
        &self,
        provider: WeakServiceProvider,
        ready: ReadySignal,
    ) -> io::Result<tokio::task::JoinHandle<HostedServiceResult>> {
        self.0.call(provider, ready)
    }
}

#[cfg(test)]
mod tests {
    use futures_util::FutureExt;
    use minfac::Registered;

    use super::*;

    /// `Some(Ok)` if `handler` is ready after creating its future, `None` if it is still pending
    fn ready_after_create<TDep, TMarker>(
        handler: impl HostedServiceFn<TDep, TMarker>,
        dep: TDep,
    ) -> Option<Result<(), futures_channel::oneshot::Canceled>> {
        let (ready, mut listener) = ReadySignal::new();
        let _future_keeps_signal_alive = handler.create(ready, dep);
        (&mut listener).now_or_never()
    }

    async fn without_signal() -> HostedServiceResult {
        Ok(())
    }

    async fn without_signal_with_dep(_: u32) -> HostedServiceResult {
        Ok(())
    }

    async fn with_signal(ready: ReadySignal) -> HostedServiceResult {
        ready.signal();
        Ok(())
    }

    async fn with_signal_and_dep(ready: ReadySignal, _: u32) -> HostedServiceResult {
        ready.signal();
        Ok(())
    }

    #[test]
    fn handlers_without_ready_signal_are_ready_when_created() {
        assert_eq!(Some(Ok(())), ready_after_create(without_signal, ()));
        assert_eq!(
            Some(Ok(())),
            ready_after_create(without_signal_with_dep, 42)
        );
    }

    #[test]
    fn handlers_with_ready_signal_are_pending_until_they_signal() {
        assert_eq!(None, ready_after_create(with_signal, ()));
        assert_eq!(None, ready_after_create(with_signal_and_dep, 42));

        let (ready, listener) = ReadySignal::new();
        let future = with_signal_and_dep.create(ready, 42);
        assert!(future.now_or_never().expect("Doesn't await").is_ok());
        assert_eq!(Some(Ok(())), listener.now_or_never());
    }

    struct Missing;

    async fn needs_missing(_: std::sync::Arc<Missing>) -> HostedServiceResult {
        Ok(())
    }

    #[test]
    fn missing_dependency_fails_build_and_names_the_service() {
        let mut c = ServiceCollection::new();
        c.with::<Registered<std::sync::Arc<Missing>>>()
            .register_hosted_service("Needs missing", needs_missing);
        let error = c.build().expect_err("Build fails").to_string();
        assert!(error.contains("needs_missing"), "{error}");
    }
}
