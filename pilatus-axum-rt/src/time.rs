use axum::{response::IntoResponse, Json};
use chrono::{DateTime, Utc};
use minfac::ServiceCollection;
use pilatus_axum::ServiceCollectionExtensions;
use serde::Serialize;

pub(super) fn register_services(c: &mut ServiceCollection) {
    #[rustfmt::skip]
    c.register_web("time", |x| x
        .http("", |m| m.get(get_time))
    );
}

async fn get_time() -> impl IntoResponse {
    #[derive(Serialize)]
    struct Response {
        utc_now: DateTime<Utc>,
    }
    Json(Response {
        utc_now: chrono::Utc::now(),
    })
}
