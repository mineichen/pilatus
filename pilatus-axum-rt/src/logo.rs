use axum::response::IntoResponse;
use axum::{extract::Query, http::header::CONTENT_TYPE};
use hyper::StatusCode;
use minfac::ServiceCollection;
use pilatus::{LogoQuery, LogoService};
use pilatus_axum::{extract::InjectRegistered, ServiceCollectionExtensions};
use tracing::trace;

pub(super) fn register_services(c: &mut ServiceCollection) {
    #[rustfmt::skip]
    c.register_web("logo", |x| x
        .http("", |m| m.get(get_logo))
    );
}

async fn get_logo(
    InjectRegistered(logo_service): InjectRegistered<LogoService>,
    Query(query): Query<LogoQuery>,
) -> impl IntoResponse {
    trace!("Get logo for Query: {query:?}");
    let logo = logo_service.get(&query);
    let mut builder = axum::http::response::Builder::new();
    if let [b'<', b'?', b'x', b'm', b'l', ..] = &logo.0[..] {
        builder = builder.header(CONTENT_TYPE, "image/svg+xml")
    }
    builder
        .body(axum::body::Body::from(logo.0.to_vec()))
        .map(IntoResponse::into_response)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Error: {e}")))
}
