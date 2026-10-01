//! The poller: every `poll-interval`, fetch upstream and publish a new tip. Fetch failures are
//! recorded on the wiki (and surface in `/status`), never fatal.

use std::sync::Arc;
use std::time::Duration;

use riki_core::wiki::Wiki;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tracing::{debug, error};

/// Run one poll, logging a store error instead of propagating it: the loop must keep going.
pub async fn poll_once(wiki: &Wiki) {
    match wiki.poll().await {
        Ok(outcome) => debug!("poll_once: {outcome:?}"),
        Err(err) => error!("poll_once: {err}"),
    }
}

/// Spawn the poll loop. The first poll runs one `interval` after the call; the startup poll is
/// the caller's.
pub fn spawn(wiki: Arc<Wiki>, interval: Duration) -> JoinHandle<()> {
    debug!("poller::spawn: interval={interval:?}");
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            poll_once(&wiki).await;
        }
    })
}

#[cfg(test)]
mod tests;
