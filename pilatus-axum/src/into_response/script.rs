use std::borrow::Cow;

use axum::response::Response;

pub struct ScriptResponse(Cow<'static, str>);

impl ScriptResponse {
    pub fn new<T: Into<Cow<'static, str>>>(input: T) -> Self {
        Self(input.into())
    }
}

impl axum::response::IntoResponse for ScriptResponse {
    fn into_response(self) -> Response {
        ([("content-type", "application/javascript")], self.0).into_response()
    }
}
