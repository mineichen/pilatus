use std::fmt::Debug;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

use pilatus::device::{ActorError, ActorMessage};

#[allow(type_alias_bounds)]
pub type DeviceMessageJsonResponse<T: ActorMessage> = DeviceJsonResponse<T::Output, T::Error>;

pub struct DeviceJsonResponse<T, TErr: Debug>(pub Result<T, ActorError<TErr>>);
pub struct DeviceResponse<T, TErr: Debug>(pub Result<T, ActorError<TErr>>);

impl<T, TErr: Debug> From<Result<T, ActorError<TErr>>> for DeviceJsonResponse<T, TErr> {
    fn from(r: Result<T, ActorError<TErr>>) -> Self {
        Self(r)
    }
}

#[derive(Debug)]
pub struct DeviceJsonError<TErr: Debug> {
    pub error: ActorError<TErr>,
}

impl<TErr: Debug> From<ActorError<TErr>> for DeviceJsonError<TErr> {
    fn from(error: ActorError<TErr>) -> Self {
        Self { error }
    }
}

impl<TErr: Debug> IntoResponse for DeviceJsonError<TErr> {
    fn into_response(self) -> axum::response::Response {
        map_actor_error_to_status_text(self.error)
    }
}

impl<T, TErr: Debug> From<Result<T, ActorError<TErr>>> for DeviceResponse<T, TErr> {
    fn from(r: Result<T, ActorError<TErr>>) -> Self {
        Self(r)
    }
}

impl<T: IntoResponse, TErr: Debug> IntoResponse for DeviceResponse<T, TErr> {
    fn into_response(self) -> axum::response::Response {
        match self.0 {
            Ok(d) => d.into_response(),
            Err(e) => map_actor_error_to_status_text(e),
        }
    }
}

impl<T: serde::Serialize, TErr: Debug> IntoResponse for DeviceJsonResponse<T, TErr> {
    fn into_response(self) -> axum::response::Response {
        match self.0 {
            Ok(d) => Json(d).into_response(),
            Err(e) => map_actor_error_to_status_text(e),
        }
    }
}

pub fn map_actor_error_to_status_text<T: Debug>(e: ActorError<T>) -> Response {
    (
        match e {
            ActorError::UnknownDevice(_) | ActorError::UnknownMessageType(_) => {
                StatusCode::NOT_FOUND
            }
            ActorError::Custom(_) => StatusCode::BAD_REQUEST,
            ActorError::Busy(_) => StatusCode::SERVICE_UNAVAILABLE,
            ActorError::Aborted => StatusCode::from_u16(499).unwrap(), // https://de.wikipedia.org/wiki/HTTP-Statuscode
            ActorError::Timeout => StatusCode::REQUEST_TIMEOUT,
            _ => StatusCode::BAD_REQUEST,
        },
        format!("{e:?}"),
    )
        .into_response()
}
