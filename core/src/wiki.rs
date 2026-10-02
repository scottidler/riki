//! The served content: the good tip's [`Published`] snapshot, the upstream health, and
//! **publish**, the only way the good tip moves.
//!
//! Publish builds the nav index for a commit. On success it builds the rest of the snapshot
//! (redirects, search), writes the commit to `refs/riki/good`, and swaps the in-memory snapshot as a unit;
//! on failure it leaves both alone and records the error. Startup reads `refs/riki/good` (the tip
//! on first run) and publishes it, so a restart never serves an invalid tip.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::Instant;

use chrono::{DateTime, SecondsFormat, Utc};
use git2::Oid;
use tracing::{debug, info, warn};

use crate::index::{ErrorList, NavIndex, label};
use crate::redirect::Redirects;
use crate::search::{PageSource, SearchIndex};
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

/// Everything served for the good tip, swapped as one unit by [`Wiki::publish`], so a request
/// never mixes the nav of one commit with the redirects or search of another. Page text lives only
/// here (in `search`), never in the per-oid nav cache.
#[derive(Debug)]
pub struct Published {
    pub nav: Arc<NavIndex>,
    pub redirects: Redirects,
    pub search: SearchIndex,
}

impl Published {
    /// The good tip this snapshot was built from.
    pub fn commit(&self) -> Oid {
        self.nav.commit()
    }
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
    good: RwLock<Option<Arc<Published>>>,
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

    /// The good tip's snapshot, or `None` when nothing has published yet.
    pub fn good(&self) -> Option<Arc<Published>> {
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
        let order_files = crate::index::order_files(&paths);
        let index = NavIndex::build(commit, paths);
        let files = index.pages().map(|(_, file)| file.to_string()).collect();
        let titles: HashMap<String, Option<String>> = self
            .store
            .read_blobs(commit, files)
            .await?
            .into_iter()
            .map(|(file, bytes)| {
                let title = crate::render::page_title(&String::from_utf8_lossy(&bytes));
                (file, title)
            })
            .collect();
        let orders = self.store.read_blobs(commit, order_files).await?;
        let (index, warnings) = index
            .with_titles(|file| titles.get(file).cloned().flatten())
            .with_orders(orders);
        for warning in &warnings {
            warn!("index: {commit}: {warning}");
        }
        let index = Arc::new(index);
        self.indexes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(commit, index.clone());
        Ok(index)
    }

    /// Publish `commit`: the only way the good tip moves, and the only writer of the served
    /// snapshot. Requires the repo mutex because it writes `refs/riki/good`. Redirects extend the
    /// current snapshot's map incrementally; the first publish (the one `open` awaits) has none,
    /// so it walks the full history before anything is served.
    pub async fn publish(&self, guard: &RepoGuard<'_>, commit: Oid) -> Result<PublishOutcome, StoreError> {
        let index = self.index(commit).await?;
        if !index.is_publishable() {
            let errors = ErrorList(index.errors()).to_string();
            warn!("publish: refusing {commit}: {errors}");
            self.health().rejected = Some(Rejected { commit, errors });
            return Ok(PublishOutcome::Refused(index));
        }
        let previous = self.good();
        let redirects = Redirects::build(&self.store, previous.as_ref().map(|good| &good.redirects), commit).await?;
        let search = self.search(&index).await?;
        self.store.set_good(guard, commit).await?;
        let published = Arc::new(Published {
            nav: index.clone(),
            redirects,
            search,
        });
        *self.good.write().unwrap_or_else(PoisonError::into_inner) = Some(published);
        self.health().rejected = None;
        info!("publish: good tip is now {commit}");
        Ok(PublishOutcome::Published(index))
    }

    /// The search index for a publishable nav: every page's blob, read in one blocking task and
    /// indexed in another. A page breaking a static rule is skipped with a WARN; store errors
    /// propagate, so publish leaves the good tip alone.
    async fn search(&self, nav: &NavIndex) -> Result<SearchIndex, StoreError> {
        let started = Instant::now();
        let commit = nav.commit();
        let mut pages: std::collections::BTreeMap<String, (String, String)> = nav
            .pages()
            .map(|(url, file)| {
                let segment = url.rsplit('/').next().unwrap_or_default();
                let title = nav
                    .node(url)
                    .map_or_else(|| segment.to_string(), |node| label(node, segment));
                (file.to_string(), (url.to_string(), title))
            })
            .collect();
        let files = pages.keys().cloned().collect();
        let blobs = self.store.read_blobs(commit, files).await?;
        let sources: Vec<PageSource> = blobs
            .into_iter()
            .filter_map(|(path, bytes)| {
                let (url, title) = pages.remove(&path)?;
                Some(PageSource {
                    path,
                    url,
                    title,
                    bytes,
                })
            })
            .collect();
        let (search, skipped) = tokio::task::spawn_blocking(move || SearchIndex::build(sources)).await?;
        for page in &skipped {
            warn!("search: {commit}: {page}");
        }
        info!(
            "search: built for {commit}: pages={} sections={} terms={} skipped={} in {:?}",
            search.page_count(),
            search.section_count(),
            search.term_count(),
            skipped.len(),
            started.elapsed()
        );
        Ok(search)
    }

    /// Fetch upstream and record the result in the health state `/status` and the banner read: a
    /// failure marks upstream unreachable (keeping the first `since`), a success clears it. Every
    /// fetch (poll and save) goes through here so neither lags the other's observation.
    pub async fn fetch(&self, guard: &RepoGuard<'_>) -> Result<(), StoreError> {
        match self.store.fetch(guard).await {
            Ok(()) => {
                if let Some(down) = self.health().unreachable.take() {
                    info!("fetch: upstream reachable again (down since {})", down.since);
                }
                Ok(())
            }
            Err(err) => {
                let error = err.to_string();
                warn!("fetch: failed: {error}");
                let mut health = self.health();
                let since = health.unreachable.as_ref().map_or_else(Utc::now, |down| down.since);
                health.unreachable = Some(Unreachable { since, error });
                Err(err)
            }
        }
    }

    /// One poll: fetch under the mutex; on a tip other than the good tip, publish it.
    pub async fn poll(&self) -> Result<PollOutcome, StoreError> {
        let guard = self.store.lock().await;
        if self.fetch(&guard).await.is_err() {
            return Ok(PollOutcome::Unreachable);
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
