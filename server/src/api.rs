//! The POST API surface and its guards. Every POST route sits behind the JSON-only guard: a
//! cross-origin JSON POST needs a CORS preflight riki never answers, so requiring
//! `application/json` blocks form-based CSRF.
//!
//! The handlers here are Phase 4 stubs (501) so the guards are testable; Phase 5 replaces the
//! save stub and Phase 6 the roundtrip stub.

use axum::Router;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use tracing::debug;

use crate::identity::Identity;
use crate::routes::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/_riki/api/page", post(save))
        .route("/_riki/api/roundtrip", post(roundtrip))
        .route_layer(middleware::from_fn(require_json))
}

/// 415 unless `Content-Type` is `application/json`; parameters (`; charset=utf-8`) are accepted.
async fn require_json(request: Request, next: Next) -> Response {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());
    if !is_json(content_type) {
        debug!("require_json: refused content-type {content_type:?}");
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Content-Type must be application/json\n",
        )
            .into_response();
    }
    next.run(request).await
}

fn is_json(content_type: Option<&str>) -> bool {
    content_type
        .and_then(|value| value.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"))
}

/// Save stub: the identity guard is real, the save itself is Phase 5.
async fn save(_: Identity) -> Response {
    StatusCode::NOT_IMPLEMENTED.into_response()
}

/// Roundtrip stub: the comparison itself is Phase 6.
async fn roundtrip() -> Response {
    StatusCode::NOT_IMPLEMENTED.into_response()
}

#[cfg(test)]
mod tests;
