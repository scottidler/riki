//! riki's HTTP shell: config, routes, and process wiring over `riki-core`.

pub mod cli;
pub mod config;
pub mod observability;
pub mod poller;
pub mod routes;

use std::sync::Arc;

use eyre::{Context, Result};
use riki_core::runtime::Runtime;
use riki_core::wiki::Wiki;
use tokio::net::TcpListener;
use tracing::info;

use crate::config::Config;

/// Bind the configured listen address.
pub async fn bind(config: &Config) -> Result<TcpListener> {
    TcpListener::bind(config.listen)
        .await
        .with_context(|| format!("binding {}", config.listen))
}

/// Serve until ctrl-c. Ready latches after the startup poll, whether or not upstream answered:
/// an unreachable upstream degrades `/status`, it does not stop riki serving the good tip.
pub async fn run(config: Config) -> Result<()> {
    let listener = bind(&config).await?;
    let runtime = Arc::new(Runtime::new());
    let store = config.store();
    let wiki = Arc::new(
        Wiki::open(&store)
            .await
            .with_context(|| format!("opening the content store at {}", store.cache_dir.display()))?,
    );
    poller::poll_once(&wiki).await;
    let poller = poller::spawn(wiki.clone(), config.git.poll_interval);
    let app = routes::router(routes::AppState::new(runtime.clone(), wiki));
    runtime.mark_ready();
    info!("riki listening on {}", listener.local_addr()?);
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("serving");
    poller.abort();
    served
}

#[cfg(test)]
mod testkit;
#[cfg(test)]
mod tests;
