//! The standard service routes plus the Kubernetes-style probes. All shaping lives in
//! `riki_core::runtime`; this file is the axum wiring and the shell's compile-time git facts.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use riki_core::runtime::{
    Build, DeployedResponse, HealthResponse, ReadyResponse, Runtime, StatusResponse, VersionResponse,
};
use riki_core::wiki::Wiki;
use tower_http::trace::TraceLayer;
use tracing::debug;

use crate::pages;

/// Compile-time git facts from this crate's `build.rs` (`env!` must resolve in the crate whose
/// build script sets it, never in core).
const BUILD: Build = Build {
    branch: env!("GIT_BRANCH"),
    revision: env!("GIT_REVISION"),
    describe: env!("GIT_DESCRIBE"),
    git_sha: env!("GIT_SHA"),
};

#[derive(Debug, Clone)]
pub struct AppState {
    pub(crate) runtime: Arc<Runtime>,
    pub(crate) wiki: Arc<Wiki>,
}

impl AppState {
    pub fn new(runtime: Arc<Runtime>, wiki: Arc<Wiki>) -> Self {
        Self { runtime, wiki }
    }
}

/// Reserved routes first, then the page catch-all. axum picks the most specific match, so content
/// can never shadow a reserved route (the nav index also refuses reserved names).
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/status", get(status))
        .route("/deployed", get(deployed))
        .route("/version", get(version))
        .route("/_riki/raw/{*path}", get(pages::raw))
        .route("/", get(pages::root))
        .route("/{*path}", get(pages::page))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(riki_core::runtime::health())
}

async fn ready(State(state): State<AppState>) -> (StatusCode, Json<ReadyResponse>) {
    let body = riki_core::runtime::ready(state.runtime.is_ready());
    let code = if body.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, Json(body))
}

async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    let error = state.wiki.status_error();
    debug!("status: error={error:?}");
    Json(riki_core::runtime::status(state.runtime.uptime_secs(), error))
}

async fn deployed(State(state): State<AppState>) -> Json<DeployedResponse> {
    Json(riki_core::runtime::deployed(state.runtime.start_system()))
}

async fn version() -> Json<VersionResponse> {
    Json(riki_core::runtime::version(&BUILD))
}

#[cfg(test)]
mod tests;
