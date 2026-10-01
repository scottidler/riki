//! riki's HTTP shell: config, routes, and process wiring over `riki-core`.

pub mod cli;
pub mod config;
pub mod observability;
pub mod routes;

use std::sync::Arc;

use eyre::{Context, Result};
use riki_core::runtime::Runtime;
use tokio::net::TcpListener;
use tracing::info;

use crate::config::Config;

/// Bind the configured listen address.
pub async fn bind(config: &Config) -> Result<TcpListener> {
    TcpListener::bind(config.listen)
        .await
        .with_context(|| format!("binding {}", config.listen))
}

/// Serve until ctrl-c.
pub async fn run(config: Config) -> Result<()> {
    let listener = bind(&config).await?;
    let runtime = Arc::new(Runtime::new());
    let app = routes::router(routes::AppState::new(runtime.clone()));
    runtime.mark_ready();
    info!("riki listening on {}", listener.local_addr()?);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("serving")
}

#[cfg(test)]
mod tests;
