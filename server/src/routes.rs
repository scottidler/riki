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
use riki_core::save::SaveSettings;
use riki_core::store::Signer;
use riki_core::wiki::Wiki;
use tower_http::trace::TraceLayer;
use tracing::debug;

use crate::config::{CommitterConfig, GitConfig, IdentityConfig};
use crate::{api, assets, pages};

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
    pub(crate) identity: Arc<IdentityConfig>,
    pub(crate) save: Arc<SaveSettings>,
    /// GitHub blob URL prefix for content files, when the remote is on GitHub.
    pub(crate) github_blob_base: Option<Arc<str>>,
}

impl AppState {
    pub fn new(runtime: Arc<Runtime>, wiki: Arc<Wiki>) -> Self {
        Self {
            runtime,
            wiki,
            identity: Arc::new(IdentityConfig::default()),
            save: Arc::new(save_settings(&CommitterConfig::default(), &GitConfig::default())),
            github_blob_base: None,
        }
    }

    /// Link content files on GitHub under this prefix (`Config::github_blob_base`).
    pub fn with_github_blob_base(mut self, base: Option<String>) -> Self {
        self.github_blob_base = base.map(Arc::from);
        self
    }

    /// Use the configured identity headers instead of the defaults.
    pub fn with_identity(mut self, identity: IdentityConfig) -> Self {
        self.identity = Arc::new(identity);
        self
    }

    /// Use these save settings instead of the defaults.
    pub fn with_save(mut self, save: SaveSettings) -> Self {
        self.save = Arc::new(save);
        self
    }
}

/// The save settings `riki-core` takes, from the `committer` and `git` config sections.
pub fn save_settings(committer: &CommitterConfig, git: &GitConfig) -> SaveSettings {
    SaveSettings {
        committer: Signer {
            name: committer.name.clone(),
            email: committer.email.clone(),
        },
        push_retries: git.push_retries,
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
        .route("/_riki/assets/{name}", get(assets::asset))
        .merge(api::router())
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
