//! Redirects after a move (design doc, Resolved Decisions): an old URL -> new URL map derived from
//! exact-rename detection over the branch's first-parent history, so riki moves and laptop
//! `git mv` both redirect, with no file in the content repo. Built inside `publish` as part of the
//! `Published` snapshot; nothing else writes it.

use std::collections::BTreeMap;
use std::time::Instant;

use git2::Oid;
use tracing::{debug, info};

use crate::index::url_for_file;
use crate::store::{GitStore, Rename, StoreError};

/// Old URL -> new URL (no leading `/`; `""` is the root), up to the commit `walked`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Redirects {
    map: BTreeMap<String, String>,
    walked: Option<Oid>,
}

impl Redirects {
    /// The map for `commit`. When `previous` was walked up to a commit on `commit`'s first-parent
    /// chain, only the commits since then are walked; otherwise (startup, a rewritten history) the
    /// full history is, and the map starts empty.
    pub async fn build(store: &GitStore, previous: Option<&Redirects>, commit: Oid) -> Result<Self, StoreError> {
        let since = previous.and_then(|previous| previous.walked);
        if since == Some(commit) {
            debug!("Redirects::build: {commit} already walked");
            return Ok(previous.cloned().unwrap_or_default());
        }
        let started = Instant::now();
        let walk = store.first_parent_renames(commit, since).await?;
        let mut redirects = match previous {
            Some(previous) if walk.reached_since => previous.clone(),
            _ => Self::default(),
        };
        redirects.apply(&walk.renames);
        redirects.walked = Some(commit);
        info!(
            "Redirects::build: {commit} {} walk of {} commits, {} renames, {} redirects in {:?}",
            if walk.reached_since { "incremental" } else { "full" },
            walk.commits,
            walk.renames.len(),
            redirects.map.len(),
            started.elapsed()
        );
        Ok(redirects)
    }

    /// Record `renames` (oldest first): each `.md` -> `.md` rename that changes the URL maps
    /// `url_for_file(from)` to `url_for_file(to)`; a later rename of the same URL replaces it.
    fn apply(&mut self, renames: &[Rename]) {
        for Rename { from, to } in renames {
            let (Some(from), Some(to)) = (url_for_file(from), url_for_file(to)) else {
                continue;
            };
            if from != to {
                self.map.insert(from, to);
            }
        }
    }

    /// Where a request for the missing URL `url` goes: follow `url -> ...` and stop at the first
    /// hop that `is_page`. `None` when the chain ends, or no hop is live within as many hops as
    /// the map has entries (a cycle of moved-away URLs).
    pub fn resolve(&self, url: &str, is_page: impl Fn(&str) -> bool) -> Option<&str> {
        let mut at = url;
        for _ in 0..self.map.len() {
            let next = self.map.get(at)?;
            if is_page(next) {
                return Some(next);
            }
            at = next;
        }
        None
    }

    /// The commit this map covers history up to.
    pub fn walked(&self) -> Option<Oid> {
        self.walked
    }

    /// Every redirect as `(old, new)`, sorted by old URL.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.map.iter().map(|(from, to)| (from.as_str(), to.as_str()))
    }
}

#[cfg(test)]
mod tests;
