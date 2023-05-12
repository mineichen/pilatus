use futures::{future::Abortable, stream::AbortRegistration, FutureExt};
use minfac::ServiceCollection;
use pilatus::device::FinalizeRecipeExecution;
use pilatus_axum::extract::ws::{Dropper, WebSocketDropperService};
use std::{
    future::pending,
    sync::{Arc, RwLock},
};

pub(super) fn register_services(c: &mut ServiceCollection) {
    let mut finalizer = c.register_shared(|| Arc::new(WsFinalizeRecipeExecution::default()));
    finalizer.alias(|x| x as Arc<dyn FinalizeRecipeExecution>);
    finalizer.alias(|x| x as Arc<dyn WebSocketDropperService>);
}

struct WsFinalizeRecipeExecution(RwLock<(Dropper, AbortRegistration)>);

impl Default for WsFinalizeRecipeExecution {
    fn default() -> Self {
        Self(std::sync::RwLock::new(Dropper::pair()))
    }
}

impl WebSocketDropperService for WsFinalizeRecipeExecution {
    fn create_dropper(&self) -> Dropper {
        let lock = self.0.read().unwrap();
        lock.0.clone()
    }
}

impl FinalizeRecipeExecution for WsFinalizeRecipeExecution {
    fn finalize_recipe_execution(&self) -> futures::future::BoxFuture<'_, ()> {
        let reg = {
            let mut lock = self.0.write().unwrap();
            let mut old = Dropper::pair();
            std::mem::swap(&mut old, &mut lock);
            old.1
        };
        async {
            Abortable::new(pending::<()>(), reg).await.ok();
        }
        .boxed()
    }
}
