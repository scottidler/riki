//! The save algorithm (design doc, API Design, steps 2-8). Step 1 (JSON and identity guards) is
//! the HTTP shell's. The repo mutex is held from the fetch (step 3) until the outcome is decided,
//! and every success path publishes, so a 200 means the next GET on this replica renders the page
//! as saved.

use git2::Oid;
use tracing::{debug, info, warn};

use crate::index::ErrorList;
use crate::page::{self, NEW_PAGE_TRAILING, StaticRule};
use crate::store::{FileCommit, PushOutcome, RepoGuard, Signer, StoreError};
use crate::wiki::Wiki;

/// What a save needs from config.
#[derive(Debug, Clone)]
pub struct SaveSettings {
    /// The git committer; the author is always the editing user.
    pub committer: Signer,
    /// How many times a non-fast-forward push goes back to the fetch before giving up with 409.
    pub push_retries: u32,
}

#[derive(Debug, Clone)]
pub struct SaveRequest {
    pub path: String,
    /// The page's blob oid as the editor loaded it; `None` for a new page.
    pub base_oid: Option<Oid>,
    /// The body without front matter.
    pub body: String,
    /// The commit message; `None` uses `riki: edit <path>`.
    pub message: Option<String>,
}

/// Every way a save ends. The HTTP shell maps each to a status code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    /// Step 8: pushed and published.
    Saved { commit: Oid },
    /// Step 5: the file already holds exactly these bytes; nothing committed.
    Unchanged,
    /// Step 4: the page moved since the editor loaded it, but to exactly these bytes (a concurrent
    /// or retried save already put them there).
    ContentPresent,
    /// Step 4: the page moved since the editor loaded it. `current_body` is the tip's body without
    /// front matter (`None` when the page is gone).
    Conflict { current_body: Option<String> },
    /// Step 7: the new commit's nav index has errors; nothing pushed.
    IndexConflict { errors: String },
    /// Step 8: still non-fast-forward after `push-retries` retries.
    RetriesExhausted { attempts: u32, line: String },
    /// Step 2: a bad path or message.
    BadRequest(String),
    /// Step 4: the tip blob breaks a static rule.
    NotEditable(StaticRule),
    /// Step 3: the fetch failed or timed out; nothing committed.
    FetchFailed(String),
    /// Step 3: upstream has no commit on the branch to save on top of.
    NoTip,
    /// Step 8: the push timed out; whether it landed is unknown, and a retry is idempotent.
    PushTimedOut(String),
    /// Step 8: any other push failure (auth, protected branch, transport); git's stderr.
    PushFailed(String),
}

/// Run the save algorithm for `request` authored by `author`.
pub async fn save(
    wiki: &Wiki,
    settings: &SaveSettings,
    author: &Signer,
    request: &SaveRequest,
) -> Result<SaveOutcome, StoreError> {
    debug!(
        "save: path={} base={:?} bytes={}",
        request.path,
        request.base_oid,
        request.body.len()
    );
    if let Err(err) = page::validate_page_path(&request.path) {
        return Ok(SaveOutcome::BadRequest(err.to_string()));
    }
    let message = match commit_message(request) {
        Ok(message) => message,
        Err(err) => return Ok(SaveOutcome::BadRequest(err)),
    };
    let store = wiki.store();
    let guard = store.lock().await;
    let mut retries = 0;
    loop {
        // Step 3.
        if let Err(err) = store.fetch(&guard).await {
            warn!("save: fetch failed: {err}");
            return Ok(SaveOutcome::FetchFailed(err.to_string()));
        }
        let Some(tip) = store.tip().await? else {
            return Ok(SaveOutcome::NoTip);
        };
        // Step 4.
        let current = store.blob_at(tip, &request.path).await?;
        let new = match &current {
            Some((_, bytes)) => match page::check_static_rules(bytes) {
                Ok(text) => {
                    let split = page::split_front_matter(text);
                    page::compose(split.front_matter, &request.body, page::trailing_newlines(text))
                }
                Err(rule) => return Ok(SaveOutcome::NotEditable(rule)),
            },
            None => page::compose("", &request.body, NEW_PAGE_TRAILING),
        };
        let current_oid = current.as_ref().map(|(oid, _)| *oid);
        let holds_new = current.as_ref().is_some_and(|(_, bytes)| *bytes == new.as_bytes());
        if current_oid != request.base_oid {
            publish_tip(wiki, &guard, tip).await?;
            if holds_new {
                info!("save: {} already holds the content at {tip}", request.path);
                return Ok(SaveOutcome::ContentPresent);
            }
            info!(
                "save: {} moved ({:?} -> {current_oid:?}), conflict",
                request.path, request.base_oid
            );
            let current_body = current.map(|(_, bytes)| current_body(&bytes));
            return Ok(SaveOutcome::Conflict { current_body });
        }
        // Step 5.
        if holds_new {
            publish_tip(wiki, &guard, tip).await?;
            return Ok(SaveOutcome::Unchanged);
        }
        // Step 6.
        let commit = store
            .commit_file(FileCommit {
                parent: tip,
                path: &request.path,
                contents: new.as_bytes(),
                author,
                committer: &settings.committer,
                message: &message,
            })
            .await?;
        // Step 7.
        let index = wiki.index(commit).await?;
        if !index.is_publishable() {
            let errors = ErrorList(index.errors()).to_string();
            warn!("save: {commit} would not publish, not pushing: {errors}");
            return Ok(SaveOutcome::IndexConflict { errors });
        }
        // Step 8.
        match store.push(&guard, commit).await {
            Ok(PushOutcome::Pushed) => {
                store.set_tip(&guard, commit).await?;
                wiki.publish(&guard, commit).await?;
                return Ok(SaveOutcome::Saved { commit });
            }
            Ok(PushOutcome::NonFastForward { line }) if retries < settings.push_retries => {
                retries += 1;
                info!(
                    "save: push rejected ({line}), retry {retries} of {}",
                    settings.push_retries
                );
            }
            Ok(PushOutcome::NonFastForward { line }) => {
                return Ok(SaveOutcome::RetriesExhausted {
                    attempts: retries + 1,
                    line,
                });
            }
            Err(err @ StoreError::Timeout { .. }) => {
                warn!("save: push outcome unknown: {err}");
                return Ok(SaveOutcome::PushTimedOut(err.to_string()));
            }
            Err(StoreError::Failed { stderr, .. }) => {
                warn!("save: push failed: {stderr}");
                return Ok(SaveOutcome::PushFailed(stderr));
            }
            Err(err) => return Err(err),
        }
    }
}

/// The user's one-line message, or `riki: edit <path>` when absent or blank.
fn commit_message(request: &SaveRequest) -> Result<String, String> {
    match request.message.as_deref().map(str::trim) {
        Some(message) if message.contains('\n') || message.contains('\r') => {
            Err("message must be one line".to_string())
        }
        Some(message) if !message.is_empty() => Ok(message.to_string()),
        _ => Ok(format!("riki: edit {}", request.path)),
    }
}

/// The body of a stored page, front matter stripped, for a 409. A blob that breaks a static rule
/// never gets here (step 4 answers 422 first), so lossy decoding only guards the type.
fn current_body(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    page::split_front_matter(&text).body.to_string()
}

async fn publish_tip(wiki: &Wiki, guard: &RepoGuard<'_>, tip: Oid) -> Result<(), StoreError> {
    if !wiki.good().is_some_and(|good| good.commit() == tip) {
        wiki.publish(guard, tip).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
