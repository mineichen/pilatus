use std::{pin::Pin, sync::Arc};

use futures::{
    future::Shared,
    stream::{AbortHandle, Abortable},
    Future, FutureExt,
};
use minfac::{Registered, ServiceCollection};
use pilatus::{SystemShutdown, SystemTerminator};

type InnerPrivateState =
    Shared<Pin<std::boxed::Box<dyn futures::Future<Output = ()> + 'static + Send + Sync>>>;

pub fn register_services(c: &mut ServiceCollection) {
    c.register_shared(|| {
        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        let x: Pin<Box<dyn Future<Output = ()> + Send + Sync>> = Box::pin(async {
            Abortable::new(tokio::signal::ctrl_c(), abort_registration)
                .await
                .ok();
        });

        Arc::new(PrivateState(abort_handle, x.shared()))
    });

    c.with::<Registered<Arc<PrivateState>>>()
        .register(|s| SystemTerminator::new(s.0.clone()));

    c.with::<Registered<Arc<PrivateState>>>()
        .register(|x| SystemShutdown::new(x.1.clone()));
}

struct PrivateState(AbortHandle, InnerPrivateState);
