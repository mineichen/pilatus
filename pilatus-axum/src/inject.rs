use anyhow::Result;
use minfac::{Resolvable, WeakServiceProvider};

use crate::{
    extract::{FromRequestParts, Inject, InjectAll, InjectRegistered},
    http::{self, request::Parts, StatusCode},
};

impl<TDep: Resolvable, S: Send + Sync> FromRequestParts<S> for Inject<TDep> {
    type Rejection = (http::StatusCode, &'static str);

    async fn from_request_parts(req: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        // Safety: during ServiceCollection::register_web(),
        // each route registers a dummy-Service which depends on all Injected resolvables
        Ok(Inject(
            get_weak_service_provider(req)?.resolve_unchecked::<TDep>(),
        ))
    }
}

impl<TDep: std::any::Any, S: Send + Sync> FromRequestParts<S> for InjectRegistered<TDep> {
    type Rejection = (http::StatusCode, &'static str);

    async fn from_request_parts(req: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(InjectRegistered(
            get_weak_service_provider(req)?
                .get::<TDep>()
                // each route registers a dummy-Service which depends on all Injected resolvables. So this is likely caught already
                .ok_or((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Should have been registered",
                ))?,
        ))
    }
}

impl<TDep: std::any::Any, S: Send + Sync> FromRequestParts<S> for InjectAll<TDep> {
    type Rejection = (http::StatusCode, &'static str);

    async fn from_request_parts(req: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        // Safety: during ServiceCollection::register_web(),
        // each route registers a dummy-Service which depends on all Injected resolvables.
        Ok(InjectAll(get_weak_service_provider(req)?.get_all::<TDep>()))
    }
}

fn get_weak_service_provider(
    req: &Parts,
) -> Result<&WeakServiceProvider, (StatusCode, &'static str)> {
    let s = req.extensions.get::<WeakServiceProvider>().ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "Middleware 'Inject' is not available. Did you forget to add this layer?",
    ))?;
    Ok(s)
}
