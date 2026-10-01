//! The served content: the good tip's nav index, the upstream health, and **publish**, the only
//! way the good tip moves.
//!
//! Publish builds the nav index for a commit. On success it writes the commit to `refs/riki/good`
//! and swaps the in-memory index; on failure it leaves both alone and records the error. Startup
//! reads `refs/riki/good` (the tip on first run), so a restart never serves an invalid tip.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use chrono::{DateTime, SecondsFormat, Utc};
use git2::Oid;
use tracing::{debug, info, warn};

use crate::index::{ErrorList, NavIndex};
use crate::store::{GitStore, RepoGuard, StoreConfig, StoreError};

/// Upstream has been unreachable since `since`; `error` is the latest fetch failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreachable {
    pub since: DateTime<Utc>,
    pub error: String,
}

impl Unreachable {
    /// `since` as RFC 3339 to the second, the form the banner and `/status` both show.
    pub fn since_text(&self) -> String {
        self.since.to_rfc3339_opts(SecondsFormat::Secs, true)
    }
}

/// The newest tip failed publish with these index errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    pub commit: Oid,
    pub errors: String,
}

#[derive(Debug, Default)]
struct Health {
    unreachable: Option<Unreachable>,
    rejected: Option<Rejected>,
}

/// What publish did with a commit.
#[derive(Debug, Clone)]
pub enum PublishOutcome {
    Published(Arc<NavIndex>),
    /// The commit's index has errors; the good tip did not move.
    Refused(Arc<NavIndex>),
}

/// What one poll did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    /// The fetch failed; the good tip is still served.
    Unreachable,
    /// The tip is the good tip (or there is no tip yet).
    Unchanged,
    Published(Oid),
    Refused(Oid),
}

#[derive(Debug)]
pub struct Wiki {
    store: GitStore,
    indexes: Mutex<HashMap<Oid, Arc<NavIndex>>>,
    good: RwLock<Option<Arc<NavIndex>>>,
    health: Mutex<Health>,
}

impl Wiki {
    /// Open the store and load what to serve: `refs/riki/good`, else (first run) the tip, else
    /// nothing until a poll fetches one. Does not touch the network.
    pub async fn open(config: &StoreConfig) -> Result<Self, StoreError> {
        let wiki = Self {
            store: GitStore::open(config).await?,
            indexes: Mutex::new(HashMap::new()),
            good: RwLock::new(None),
            health: Mutex::new(Health::default()),
        };
        let start = match wiki.store.good().await? {
            Some(good) => Some(good),
            None => wiki.store.tip().await?,
        };
        info!("Wiki::open: starting from {start:?}");
        if let Some(commit) = start {
            let guard = wiki.store.lock().await;
            wiki.publish(&guard, commit).await?;
        }
        Ok(wiki)
    }

    pub fn store(&self) -> &GitStore {
        &self.store
    }

    /// The good tip's index, or `None` when nothing has published yet.
    pub fn good(&self) -> Option<Arc<NavIndex>> {
        self.good.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn unreachable(&self) -> Option<Unreachable> {
        self.health().unreachable.clone()
    }

    pub fn rejected(&self) -> Option<Rejected> {
        self.health().rejected.clone()
    }

    /// The `/status` error: set while upstream is unreachable or the newest tip failed publish.
    pub fn status_error(&self) -> Option<String> {
        let health = self.health();
        let mut parts = Vec::new();
        if let Some(down) = &health.unreachable {
            parts.push(format!(
                "upstream unreachable since {}: {}",
                down.since_text(),
                down.error
            ));
        }
        if let Some(rejected) = &health.rejected {
            parts.push(format!("tip {} failed publish: {}", rejected.commit, rejected.errors));
        }
        (!parts.is_empty()).then(|| parts.join("; "))
    }

    /// The nav index for `commit`, built once and cached by oid.
    pub async fn index(&self, commit: Oid) -> Result<Arc<NavIndex>, StoreError> {
        if let Some(index) = self.indexes.lock().unwrap_or_else(PoisonError::into_inner).get(&commit) {
            return Ok(index.clone());
        }
        let paths = self.store.blob_paths(commit).await?;
        let index = Arc::new(NavIndex::build(commit, paths));
        self.indexes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(commit, index.clone());
        Ok(index)
    }

    /// Publish `commit`: the only way the good tip moves. Requires the repo mutex because it
    /// writes `refs/riki/good`.
    pub async fn publish(&self, guard: &RepoGuard<'_>, commit: Oid) -> Result<PublishOutcome, StoreError> {
        let index = self.index(commit).await?;
        if !index.is_publishable() {
            let errors = ErrorList(index.errors()).to_string();
            warn!("publish: refusing {commit}: {errors}");
            self.health().rejected = Some(Rejected { commit, errors });
            return Ok(PublishOutcome::Refused(index));
        }
        self.store.set_good(guard, commit).await?;
        *self.good.write().unwrap_or_else(PoisonError::into_inner) = Some(index.clone());
        self.health().rejected = None;
        info!("publish: good tip is now {commit}");
        Ok(PublishOutcome::Published(index))
    }

    /// One poll: fetch under the mutex; on a tip other than the good tip, publish it.
    pub async fn poll(&self) -> Result<PollOutcome, StoreError> {
        let guard = self.store.lock().await;
        if let Err(err) = self.store.fetch(&guard).await {
            let error = err.to_string();
            warn!("poll: fetch failed: {error}");
            let mut health = self.health();
            let since = health.unreachable.as_ref().map_or_else(Utc::now, |down| down.since);
            health.unreachable = Some(Unreachable { since, error });
            return Ok(PollOutcome::Unreachable);
        }
        if let Some(down) = self.health().unreachable.take() {
            info!("poll: upstream reachable again (down since {})", down.since);
        }
        let Some(tip) = self.store.tip().await? else {
            debug!("poll: no tip yet");
            return Ok(PollOutcome::Unchanged);
        };
        if self.good().is_some_and(|good| good.commit() == tip) {
            self.health().rejected = None;
            return Ok(PollOutcome::Unchanged);
        }
        Ok(match self.publish(&guard, tip).await? {
            PublishOutcome::Published(_) => PollOutcome::Published(tip),
            PublishOutcome::Refused(_) => PollOutcome::Refused(tip),
        })
    }

    fn health(&self) -> std::sync::MutexGuard<'_, Health> {
        self.health.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests;
