use std::marker::PhantomData;

use axum::handler::Handler;
use minfac::ServiceCollection;

use super::DependencyProvider;

pub struct Router {
    prefix: &'static str,
    pub(crate) axum_router: axum::Router,
    pub(crate) dependencies: Vec<fn(&mut ServiceCollection)>,
}
impl Router {
    pub(crate) fn new(prefix: &'static str) -> Self {
        Self {
            prefix,
            axum_router: Default::default(),
            dependencies: Default::default(),
        }
    }
    pub fn http(
        mut self,
        path: &'static str,
        f: fn(MethodRouter<()>) -> MethodRouter<()>,
    ) -> Router {
        let MethodRouter(axum_method_router, deps) = f(MethodRouter::new());

        if path.contains(':') {
            panic!("Axum changed its path-parameters from ':foo' to '{{foo}}': {path}");
        }

        if path.contains("*") && !path.contains("{*") {
            panic!("Axum changed its wildcard-parameters from '*foo' to '{{*foo}}': {path}");
        }

        self.axum_router = self
            .axum_router
            .route(&format!("/{}{path}", self.prefix), axum_method_router);
        self.dependencies.extend(deps);
        self
    }
}

pub struct MethodRouter<S>(
    axum::routing::MethodRouter<S>,
    Vec<fn(&mut ServiceCollection)>,
);

impl<S: Send + Sync + 'static + Clone> MethodRouter<S> {
    fn new() -> Self {
        Self(Default::default(), Default::default())
    }
    pub fn get<T: 'static + DependencyProvider, H: Handler<T, S>>(mut self, handler: H) -> Self {
        self.0 = self.0.get(handler);
        self.1.push(|c: &mut ServiceCollection| {
            c.with::<T::Dep>().register(|_| PhantomData::<T>);
        });
        self
    }
    pub fn post<T: 'static + DependencyProvider, H: Handler<T, S>>(mut self, handler: H) -> Self {
        self.0 = self.0.post(handler);
        self.1.push(|c: &mut ServiceCollection| {
            c.with::<T::Dep>().register(|_| PhantomData::<T>);
        });
        self
    }
    pub fn put<T: 'static + DependencyProvider, H: Handler<T, S>>(mut self, handler: H) -> Self {
        self.0 = self.0.put(handler);
        self.1.push(|c: &mut ServiceCollection| {
            c.with::<T::Dep>().register(|_| PhantomData::<T>);
        });
        self
    }
    pub fn delete<T: 'static + DependencyProvider, H: Handler<T, S>>(mut self, handler: H) -> Self {
        self.0 = self.0.delete(handler);
        self.1.push(|c: &mut ServiceCollection| {
            c.with::<T::Dep>().register(|_| PhantomData::<T>);
        });
        self
    }
}
