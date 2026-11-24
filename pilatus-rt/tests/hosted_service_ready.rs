//! The runtime polls the future of `run`, `run_until_finished` and `run_until` once all HostedServices are ready.
//! Failing HostedServices are returned as [`RunUntilError`] in the error type of the caller.
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use minfac::{Registered, ServiceCollection};
use pilatus::prelude::*;
use pilatus::{ReadySignal, Recipe, RecipeService, SystemShutdown, SystemTerminator};
use pilatus_rt::{RecipeServiceImpl, RunUntilError, Runtime, TempRuntime};
use testresult::TestResult;

#[derive(Default)]
struct Probe {
    ticks: AtomicUsize,
    slow_is_ready: AtomicBool,
}

impl Probe {
    fn ticks(&self) -> usize {
        self.ticks.load(Ordering::SeqCst)
    }

    /// Fails, if the ticker doesn't tick anymore
    async fn wait_for_next_tick(&self) -> Result<(), tokio::time::error::Elapsed> {
        let ticks = self.ticks();
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.ticks() == ticks {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
    }
}

type Deps = (Arc<Probe>, SystemShutdown);

/// Is ready immediately and keeps ticking afterwards
async fn ticker(ready: ReadySignal, (probe, mut shutdown): Deps) -> anyhow::Result<()> {
    ready.signal();
    let _shutdown = shutdown
        .run_or_shutdown(async {
            loop {
                probe.ticks.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await;
    Ok(())
}

/// Is only ready after the ticker ran past its ready signal
async fn slow(ready: ReadySignal, (probe, shutdown): Deps) -> anyhow::Result<()> {
    while probe.ticks() < 5 {
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    probe.slow_is_ready.store(true, Ordering::SeqCst);
    ready.signal();
    shutdown.await;
    Ok(())
}

/// Services without ReadySignal are ready immediately
async fn legacy(shutdown: SystemShutdown) -> anyhow::Result<()> {
    shutdown.await;
    Ok(())
}

async fn legacy_without_dependencies() -> anyhow::Result<()> {
    Ok(())
}

extern "C" fn register_services(c: &mut ServiceCollection) {
    c.with::<(Registered<Arc<Probe>>, Registered<SystemShutdown>)>()
        .register_hosted_service("Ticker", ticker);
    c.with::<(Registered<Arc<Probe>>, Registered<SystemShutdown>)>()
        .register_hosted_service("Slow", slow);
    c.with::<Registered<SystemShutdown>>()
        .register_hosted_service("Legacy", legacy);
    c.register_hosted_service("Legacy without dependencies", legacy_without_dependencies);
}

#[test]
fn run_until_waits_for_all_hosted_services() -> TestResult {
    TempRuntime::new()
        .register(register_services)
        .register_instance(Arc::new(Probe::default()))
        .configure()?
        .run_until(|Registered(probe): Registered<Arc<Probe>>| async move {
            assert!(
                probe.slow_is_ready.load(Ordering::SeqCst),
                "run_until started before 'Slow' was ready"
            );
            // Ticker keeps running after all services are ready
            probe.wait_for_next_tick().await?;
            Ok(())
        })
}

#[test]
fn run_polls_other_once_all_hosted_services_are_ready() -> TestResult {
    let dir = tempfile::tempdir()?;
    let probe = Arc::new(Probe::default());
    let runtime = Runtime::with_root(dir.path())
        .register(register_services)
        .register_instance(probe.clone())
        .configure();
    let terminator = runtime
        .provider
        .get::<SystemTerminator>()
        .expect("Registered by pilatus-rt");
    // `run` doesn't shut down by itself
    runtime.run(async move {
        assert!(
            probe.slow_is_ready.load(Ordering::SeqCst),
            "run polled its future before 'Slow' was ready"
        );
        terminator.shutdown();
        Ok(())
    })
}

/// `anyhow::Error` is used as error type to inspect the [`RunUntilError`]
fn run_until_error(result: anyhow::Result<impl Debug>) -> RunUntilError<anyhow::Error> {
    result
        .expect_err("run_until fails")
        .downcast()
        .expect("anyhow::Error contains the RunUntilError")
}

#[test]
fn errors_of_until_are_returned() -> TestResult {
    let result = TempRuntime::new()
        .configure()?
        .run_until(|()| async { Err::<(), _>(anyhow::anyhow!("Until failed")) });
    let RunUntilError::Until(until) = run_until_error(result) else {
        panic!("Expected RunUntilError::Until");
    };
    assert_eq!("Until failed", until.to_string());
    Ok(())
}

async fn fails_before_ready(_ready: ReadySignal) -> anyhow::Result<()> {
    tokio::time::sleep(Duration::from_millis(10)).await;
    anyhow::bail!("Failed during startup")
}

async fn fails_on_shutdown(shutdown: SystemShutdown) -> anyhow::Result<()> {
    shutdown.await;
    anyhow::bail!("Failed on shutdown")
}

extern "C" fn register_failing_services(c: &mut ServiceCollection) {
    c.with::<(Registered<Arc<Probe>>, Registered<SystemShutdown>)>()
        .register_hosted_service("Ticker", ticker);
    c.register_hosted_service("Fails before ready", fails_before_ready);
    c.with::<Registered<SystemShutdown>>()
        .register_hosted_service("Fails on shutdown", fails_on_shutdown);
}

#[test]
fn failing_services_dont_stop_the_runtime_and_are_returned_once_all_stopped() -> TestResult {
    let result = TempRuntime::new()
        .register(register_failing_services)
        .register_instance(Arc::new(Probe::default()))
        .configure()?
        .run_until(|Registered(probe): Registered<Arc<Probe>>| async move {
            // 'Fails before ready' failed already, as it isn't waited for anymore
            probe.wait_for_next_tick().await?;
            Err::<(), _>(anyhow::anyhow!("Until failed"))
        });

    let RunUntilError::HostedServiceAndUntil(services, until) = run_until_error(result) else {
        panic!("Expected RunUntilError::HostedServiceAndUntil");
    };
    assert_eq!("Until failed", until.to_string());
    let errors = services
        .errors()
        .iter()
        .map(|e| format!("{e:#}"))
        .collect::<Vec<_>>();
    assert_eq!(
        [
            "HostedService 'Fails before ready' failed: Failed during startup",
            "HostedService 'Fails on shutdown' failed: Failed on shutdown"
        ],
        errors.as_slice()
    );
    Ok(())
}

extern "C" fn register_service_failing_before_ready(c: &mut ServiceCollection) {
    c.register_hosted_service("Fails before ready", fails_before_ready);
}

#[test]
fn services_which_fail_before_they_are_ready_are_not_waited_for() -> TestResult {
    let result = TempRuntime::new()
        .register(register_service_failing_before_ready)
        .configure()?
        .run_until(|()| async { anyhow::Ok(42) });
    let RunUntilError::HostedService(services) = run_until_error(result) else {
        panic!("Expected RunUntilError::HostedService");
    };
    assert_eq!(1, services.errors().len());
    Ok(())
}

/// Stops without signaling, but its ReadySignal stays alive
async fn leaks_ready_signal_and_stops(ready: ReadySignal) -> anyhow::Result<()> {
    std::mem::forget(ready);
    Ok(())
}

extern "C" fn register_leaking_and_stopping_service(c: &mut ServiceCollection) {
    c.register_hosted_service("Leaks ReadySignal and stops", leaks_ready_signal_and_stops);
}

#[test]
fn run_until_does_not_wait_for_stopped_services_with_outliving_ready_signal() -> TestResult {
    let runtime = TempRuntime::new()
        .register(register_leaking_and_stopping_service)
        .configure()?;
    let (tx, rx) = std::sync::mpsc::channel();
    // Runs in a separate thread, so a regression fails instead of blocking the test forever
    std::thread::spawn(move || {
        let _ignore_timed_out_test = tx.send(runtime.run_until(|()| async { TestResult::Ok(42) }));
    });
    assert_eq!(42, rx.recv_timeout(Duration::from_secs(10))??);
    Ok(())
}

#[test]
fn device_runner_accepts_recipe_changes_immediately() -> TestResult {
    TempRuntime::new().configure()?.run_until(
        |(Registered(service), Registered(rs)): (
            Registered<RecipeService>,
            Registered<Arc<RecipeServiceImpl>>,
        )| async move {
            let id = rs.add_recipe(Recipe::default()).await?;
            service.start_recipe(id.clone()).await?;
            assert_eq!(id, rs.get_active_id().await);
            Ok(())
        },
    )
}
