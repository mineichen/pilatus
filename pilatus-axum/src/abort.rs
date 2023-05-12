use std::fmt::Debug;

use async_trait::async_trait;
use axum::http::StatusCode;
use futures::stream::{AbortRegistration, Abortable};
use pilatus::device::ActorError;
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

use super::{
    extract::{FromRequestParts, InjectRegistered, Query},
    http,
    http::request::Parts,
};

pub struct Abort(pub Option<AbortRegistration>);

impl Abort {
    pub async fn make_cancellable<TOk, TErr: Debug>(
        self,
        fut: impl std::future::Future<Output = Result<TOk, ActorError<TErr>>>,
    ) -> Result<TOk, ActorError<TErr>> {
        match self.0 {
            Some(abort) => Abortable::new(fut, abort).await?,
            None => fut.await,
        }
    }
}

pub struct AbortServiceInterface(pub Box<dyn Fn(Uuid) -> Option<AbortRegistration>>);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for Abort {
    type Rejection = (http::StatusCode, &'static str);

    async fn from_request_parts(req: &mut Parts, s: &S) -> Result<Self, Self::Rejection> {
        #[derive(Deserialize)]
        struct AbortIdRequest {
            abort_id: Uuid,
        }

        if let Ok(Query(AbortIdRequest { abort_id })) = Query::from_request_parts(req, s).await {
            let InjectRegistered(s) =
                InjectRegistered::<AbortServiceInterface>::from_request_parts(req, s).await?;
            match (s.0)(abort_id) {
                Some(x) => Ok(Abort(Some(x))),
                None => {
                    info!("Abort id {} was used already", abort_id);
                    Err((StatusCode::BAD_REQUEST, "Abort id was used already"))
                }
            }
        } else {
            Ok(Abort(None))
        }
    }
}
