//! The write driver: steps 3, 7, 8 of the save algorithm (design doc, API Design), shared by every
//! op. The repo mutex is held once, from the first fetch until the outcome is decided; the op
//! supplies its own check-and-build (steps 4-6) against each fetched tip. Every answer the op gives
//! without a commit publishes the fetched tip first, so a 200 means the next GET on this replica
//! renders the post-op state.

use git2::Oid;
use std::future::Future;
use tracing::{info, warn};

use crate::index::ErrorList;
use crate::save::SaveSettings;
use crate::store::{GitStore, PushOutcome, RepoGuard, Signer, StoreError};
use crate::wiki::{PublishOutcome, Wiki};

/// What an op's check-and-build sees: the freshly fetched tip and who signs a commit on it.
pub struct Fetched<'a> {
    pub store: &'a GitStore,
    pub tip: Oid,
    pub author: &'a Signer,
    pub committer: &'a Signer,
}

impl Fetched<'_> {
    /// The folder of `path` that is a file at the tip, so `path` cannot be created there: git2's
    /// tree builder would fail with a D/F conflict, which an op answers as a 409 instead.
    pub async fn file_ancestor<'p>(&self, path: &'p str) -> Result<Option<&'p str>, StoreError> {
        for dir in ancestors(path) {
            if self.store.blob_at(self.tip, dir).await?.is_some() {
                return Ok(Some(dir));
            }
        }
        Ok(None)
    }
}

/// Every proper directory prefix of `file`: `a/b/c.md` -> `a`, `a/b`.
pub(crate) fn ancestors(file: &str) -> impl Iterator<Item = &str> {
    file.match_indices('/').map(|(at, _)| &file[..at])
}

/// The op's answer for one fetched tip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check<T> {
    /// Steps 4-6 passed and built this commit on the tip; the driver validates and pushes it.
    Committed(Oid),
    /// A 200 without a commit (unchanged, content already present). The driver publishes the tip
    /// first and answers the index conflict instead if the tip won't publish.
    NoCommit(T),
    /// The tip moved under the client. The driver publishes the tip (so the client's reload sees
    /// it) and answers `T` whether or not it published.
    Conflict(T),
    /// The tip refuses the op outright (e.g. a page breaking a static rule); nothing is published.
    Refused(T),
}

/// One write request's steps 4-6.
pub trait WriteOp {
    /// The op's own answers, carried through `WriteOutcome::Op`.
    type Outcome;

    /// Check the op against `at.tip` and, when it may proceed, build its one commit on that tip.
    fn check_and_build(
        &self,
        at: &Fetched<'_>,
    ) -> impl Future<Output = Result<Check<Self::Outcome>, StoreError>> + Send;
}

/// Every way a write ends. Each op maps these onto its own outcome type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome<T> {
    /// Step 8: pushed and published.
    Pushed { commit: Oid },
    /// The op's answer without a commit, after the tip was published (or refused by the op).
    Op(T),
    /// Step 7 (the new commit) or a no-commit 200 (the fetched tip): the nav index has errors;
    /// nothing pushed.
    IndexConflict { errors: String },
    /// Step 8: still non-fast-forward after `push-retries` retries.
    RetriesExhausted { attempts: u32, line: String },
    /// Step 3: the fetch failed or timed out; nothing committed.
    FetchFailed(String),
    /// Step 3: upstream has no commit on the branch to write on top of.
    NoTip,
    /// Step 8: the push timed out; whether it landed is unknown, and a retry is idempotent.
    PushTimedOut(String),
    /// Step 8: any other push failure (auth, protected branch, transport); git's stderr.
    PushFailed(String),
}

/// What a path op (delete, restore, move) answers when it makes no commit of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpAnswer {
    /// The tip already holds the op's result (a retried or concurrent op got there first); the
    /// tip was published before this answer.
    ContentPresent,
    /// The tip moved under the client; the message says how.
    Conflict(String),
    /// The request can never succeed as sent: a bad path, a guard, an unrecognized commit.
    BadRequest(String),
}

/// The user's one-line commit message, or `default` when absent or blank.
pub fn one_line_message(message: Option<&str>, default: impl FnOnce() -> String) -> Result<String, String> {
    match message.map(str::trim) {
        Some(message) if message.contains('\n') || message.contains('\r') => {
            Err("message must be one line".to_string())
        }
        Some(message) if !message.is_empty() => Ok(message.to_string()),
        _ => Ok(default()),
    }
}

/// Run `op` authored by `author`: fetch, check-and-build, validate, push, retry on
/// non-fast-forward, publish, all under one hold of the repo mutex.
pub async fn run<O: WriteOp>(
    wiki: &Wiki,
    settings: &SaveSettings,
    author: &Signer,
    op: &O,
) -> Result<WriteOutcome<O::Outcome>, StoreError> {
    let store = wiki.store();
    let guard = store.lock().await;
    let mut retries = 0;
    loop {
        // Step 3.
        if let Err(err) = wiki.fetch(&guard).await {
            warn!("write: fetch failed: {err}");
            return Ok(WriteOutcome::FetchFailed(err.to_string()));
        }
        let Some(tip) = store.tip().await? else {
            return Ok(WriteOutcome::NoTip);
        };
        // Steps 4-6.
        let fetched = Fetched {
            store,
            tip,
            author,
            committer: &settings.committer,
        };
        let commit = match op.check_and_build(&fetched).await? {
            Check::Committed(commit) => commit,
            Check::NoCommit(answer) => {
                if let Some(errors) = publish_tip(wiki, &guard, tip).await? {
                    return Ok(WriteOutcome::IndexConflict { errors });
                }
                return Ok(WriteOutcome::Op(answer));
            }
            Check::Conflict(answer) => {
                publish_tip(wiki, &guard, tip).await?;
                return Ok(WriteOutcome::Op(answer));
            }
            Check::Refused(answer) => return Ok(WriteOutcome::Op(answer)),
        };
        // Step 7.
        let index = wiki.index(commit).await?;
        if !index.is_publishable() {
            let errors = ErrorList(index.errors()).to_string();
            warn!("write: {commit} would not publish, not pushing: {errors}");
            return Ok(WriteOutcome::IndexConflict { errors });
        }
        // Step 8.
        match store.push(&guard, commit).await {
            Ok(PushOutcome::Pushed) => {
                store.set_tip(&guard, commit).await?;
                wiki.publish(&guard, commit).await?;
                return Ok(WriteOutcome::Pushed { commit });
            }
            Ok(PushOutcome::NonFastForward { line }) if retries < settings.push_retries => {
                retries += 1;
                info!(
                    "write: push rejected ({line}), retry {retries} of {}",
                    settings.push_retries
                );
            }
            Ok(PushOutcome::NonFastForward { line }) => {
                return Ok(WriteOutcome::RetriesExhausted {
                    attempts: retries + 1,
                    line,
                });
            }
            Err(err @ StoreError::Timeout { .. }) => {
                warn!("write: push outcome unknown: {err}");
                return Ok(WriteOutcome::PushTimedOut(err.to_string()));
            }
            Err(StoreError::Failed { stderr, .. }) => {
                warn!("write: push failed: {stderr}");
                return Ok(WriteOutcome::PushFailed(stderr));
            }
            Err(err) => return Err(err),
        }
    }
}

/// Publish the fetched tip unless it is already the good tip. `Some(errors)` when the tip's nav
/// index refuses publish: the good tip did not move, so no 200 may be returned on top of it.
async fn publish_tip(wiki: &Wiki, guard: &RepoGuard<'_>, tip: Oid) -> Result<Option<String>, StoreError> {
    if wiki.good().is_some_and(|good| good.commit() == tip) {
        return Ok(None);
    }
    Ok(match wiki.publish(guard, tip).await? {
        PublishOutcome::Published(_) => None,
        PublishOutcome::Refused(index) => {
            let errors = ErrorList(index.errors()).to_string();
            warn!("write: upstream tip {tip} refuses publish: {errors}");
            Some(errors)
        }
    })
}

#[cfg(test)]
mod tests;
