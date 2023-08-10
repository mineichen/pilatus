use std::fmt::Debug;

use futures::{Future, TryFutureExt};

use crate::device::ActorError;

#[cfg(feature = "rayon")]
pub fn execute_blocking<TOk: Send + 'static, TErr: Send + 'static + Debug>(
    f: impl FnOnce() -> Result<TOk, TErr> + Send + 'static,
) -> impl Future<Output = Result<TOk, ActorError<TErr>>> {
    use futures::channel::oneshot;

    let (tx, rx) = oneshot::channel();
    rayon::spawn(move || {
        let result = (f)();
        //trace!("Calculated result");
        let _ignore_abortion = tx.send(result);
    });
    rx.unwrap_or_else(|_| panic!("Sender is never dropped"))
        .map_err(ActorError::custom)
}
#[cfg(not(feature = "rayon"))]
pub fn execute_blocking<TOk: Send + 'static, TErr: Send + 'static + Debug>(
    f: impl FnOnce() -> Result<TOk, TErr> + Send + 'static,
) -> impl Future<Output = Result<TOk, ActorError<TErr>>> {
    tokio::task::spawn_blocking(f)
        .map_err(Into::into)
        .and_then(|x| async { x.map_err(ActorError::custom) })
}
