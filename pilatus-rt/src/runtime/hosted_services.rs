//! Spawns all [`HostedService`]s and supervises them until they stopped.
//!
//! Each service is [`State::Starting`] until it is ready or stopped, then [`State::Running`] until it stopped.
//! Failures are logged immediately and reported, once all services stopped.

use std::{
    fmt::{self, Display},
    future::Future,
    pin::{pin, Pin},
    task::{ready, Context, Poll},
    time::Duration,
};

use futures::{
    channel::oneshot::Canceled,
    future::{select, Either, Select},
    stream::FuturesUnordered,
    FutureExt, StreamExt,
};
use minfac::ServiceProvider;
use pilatus::{HostedService, ReadyListener, ReadySignal};
use tokio::task::{JoinError, JoinHandle};
use tracing::{debug, error, info, warn};

/// If no HostedService became ready or stopped within this duration, the pending ones are logged
const READY_WARNING_INTERVAL: Duration = Duration::from_secs(5);

type Handle = JoinHandle<anyhow::Result<()>>;
type ServiceResult = Result<anyhow::Result<()>, JoinError>;

/// One or more [`HostedService`]s failed. Each error has the name of its service as context.
#[derive(Debug)]
pub struct HostedServicesError(Vec<anyhow::Error>);

impl HostedServicesError {
    /// Errors of all failed services. Contains at least one
    pub fn errors(&self) -> &[anyhow::Error] {
        &self.0
    }

    pub fn into_errors(self) -> Vec<anyhow::Error> {
        self.0
    }
}

impl Display for HostedServicesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut errors = self.0.iter();
        if let Some(first) = errors.next() {
            write!(f, "{first:#}")?;
        }
        for error in errors {
            write!(f, "; {error:#}")?;
        }
        Ok(())
    }
}

impl std::error::Error for HostedServicesError {}

pub(super) struct HostedServices {
    services: FuturesUnordered<Service>,
    /// Number of services, which are neither ready nor stopped
    starting: usize,
    errors: Vec<anyhow::Error>,
}

impl HostedServices {
    /// Has to be called within the tokio runtime
    pub(super) fn spawn(provider: &ServiceProvider) -> Self {
        let mut errors = Vec::new();
        let services: FuturesUnordered<_> = provider
            .get_all::<HostedService>()
            .filter_map(|service| {
                let name = service.get_name();
                let (ready, listener) = ReadySignal::new();
                match service.call(provider.into(), ready) {
                    Ok(handle) => Some(Service {
                        name,
                        state: State::Starting(select(handle, listener)),
                    }),
                    Err(e) => {
                        let e = anyhow::Error::from(e)
                            .context(format!("Failed to spawn HostedService '{name}'"));
                        error!("{e:?}");
                        errors.push(e);
                        None
                    }
                }
            })
            .collect();
        Self {
            starting: services.len(),
            services,
            errors,
        }
    }

    /// Returns once each service is ready or stopped. Pending services are logged every [`READY_WARNING_INTERVAL`]
    pub(super) async fn wait_until_ready(&mut self) {
        let mut warning = tokio::time::interval_at(
            tokio::time::Instant::now() + READY_WARNING_INTERVAL,
            READY_WARNING_INTERVAL,
        );
        while self.starting > 0 {
            match select(self.services.next(), pin!(warning.tick())).await {
                Either::Left((Some((name, event)), _)) => self.handle(name, event),
                Either::Left((None, _)) => unreachable!("Starting services are in `services`"),
                Either::Right(_) => {
                    let pending = self
                        .services
                        .iter()
                        .filter(|s| matches!(s.state, State::Starting(_)))
                        .map(|s| s.name)
                        .collect::<Vec<_>>();
                    warn!("Waiting for HostedServices to be ready: {pending:?}");
                }
            }
        }
        info!("All HostedServices are ready");
    }

    /// Returns once all services stopped
    pub(super) async fn wait_until_stopped(mut self) -> Result<(), HostedServicesError> {
        while let Some((name, event)) = self.services.next().await {
            self.handle(name, event);
        }
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(HostedServicesError(self.errors))
        }
    }

    fn handle(&mut self, name: &'static str, event: Event) {
        match event {
            Event::Ready(signal, handle) => {
                self.starting -= 1;
                match signal {
                    Ok(()) => debug!("HostedService '{name}' is ready"),
                    Err(Canceled) => debug!(
                        "HostedService '{name}' dropped its ReadySignal without signaling. It isn't waited for anymore"
                    ),
                }
                self.services.push(Service {
                    name,
                    state: State::Running(handle),
                });
            }
            Event::StoppedStarting(result) => {
                self.starting -= 1;
                self.stopped(name, result);
            }
            Event::Stopped(result) => self.stopped(name, result),
        }
    }

    fn stopped(&mut self, name: &'static str, result: ServiceResult) {
        match result.map_err(anyhow::Error::from).and_then(|r| r) {
            Ok(()) => {
                let mut remaining = self.services.iter();
                if let (Some(last), None) = (remaining.next(), remaining.next()) {
                    let last = last.name;
                    info!("HostedService '{name}' stopped. Just service '{last}' is remaining");
                } else {
                    let count = self.services.len();
                    info!("HostedService '{name}' stopped. '{count}' remaining");
                }
            }
            Err(e) => {
                let e = e.context(format!("HostedService '{name}' failed"));
                error!("{e:?}");
                self.errors.push(e);
            }
        }
    }
}

/// Future of a spawned [`HostedService`]
struct Service {
    name: &'static str,
    state: State,
}

enum State {
    /// Resolves once the service is ready or stopped.
    /// The handle is polled first: A stopped service isn't waited for, even if its ReadySignal still exists (e.g. moved into a detached task)
    Starting(Select<Handle, ReadyListener>),
    /// Resolves once the service stopped
    Running(Handle),
}

enum Event {
    /// The service signaled ready (`Ok`) or dropped its ReadySignal without signaling (`Err`). It keeps running
    Ready(Result<(), Canceled>, Handle),
    /// The service stopped before it was ready
    StoppedStarting(ServiceResult),
    /// The service stopped after it was ready
    Stopped(ServiceResult),
}

impl Future for Service {
    type Output = (&'static str, Event);

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let event = match &mut this.state {
            State::Starting(starting) => match ready!(starting.poll_unpin(cx)) {
                Either::Left((result, _listener)) => Event::StoppedStarting(result),
                Either::Right((signal, handle)) => Event::Ready(signal, handle),
            },
            State::Running(handle) => Event::Stopped(ready!(handle.poll_unpin(cx))),
        };
        Poll::Ready((this.name, event))
    }
}
