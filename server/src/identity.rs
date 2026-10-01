//! Header identity: the edge (Authelia via Caddy) authenticates and sets the email and name
//! headers; riki trusts them only because header mode requires a loopback listen
//! (`Config::validate`), so nothing but the local edge can reach it.

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use tracing::debug;

use crate::config::IdentityConfig;
use crate::routes::AppState;

/// The editing user: becomes the git author of a save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub email: String,
    pub name: String,
}

impl Identity {
    /// Read the identity from request headers. The email header is required; a missing, empty,
    /// or non-UTF-8 value is `None` (the caller answers 401). The name header is optional and
    /// falls back to the email.
    pub fn from_headers(headers: &axum::http::HeaderMap, config: &IdentityConfig) -> Option<Self> {
        let value = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        let email = value(&config.email_header)?;
        let name = value(&config.name_header).unwrap_or_else(|| email.clone());
        Some(Self { email, name })
    }
}

/// Rejection: 401, the edge did not vouch for this request.
#[derive(Debug)]
pub struct Unauthenticated;

impl IntoResponse for Unauthenticated {
    fn into_response(self) -> Response {
        (
            StatusCode::UNAUTHORIZED,
            "authentication required: identity header missing\n",
        )
            .into_response()
    }
}

impl FromRequestParts<AppState> for Identity {
    type Rejection = Unauthenticated;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let identity = Identity::from_headers(&parts.headers, &state.identity);
        debug!("Identity::from_request_parts: present={}", identity.is_some());
        identity.ok_or(Unauthenticated)
    }
}

#[cfg(test)]
mod tests;
