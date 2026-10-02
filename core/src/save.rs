//! The save algorithm (design doc, API Design, steps 2-8) as the `Edit` op. Step 1 (JSON and
//! identity guards) is the HTTP shell's; step 2 is here; steps 3, 7, 8 are the write driver's
//! (`crate::write`); steps 4-6 are `Edit::check_and_build`.

use git2::Oid;
use tracing::{debug, info};

use crate::page::{self, NEW_PAGE_TRAILING, StaticRule};
use crate::store::{FileCommit, Signer, StoreError};
use crate::wiki::Wiki;
use crate::write::{self, Check, Fetched, WriteOp, WriteOutcome};

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

/// Run the save algorithm for `request` authored by `author`: the `Edit` op on the write driver.
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
    let op = Edit { request, message };
    Ok(match write::run(wiki, settings, author, &op).await? {
        WriteOutcome::Pushed { commit } => SaveOutcome::Saved { commit },
        WriteOutcome::Op(outcome) => outcome,
        WriteOutcome::IndexConflict { errors } => SaveOutcome::IndexConflict { errors },
        WriteOutcome::RetriesExhausted { attempts, line } => SaveOutcome::RetriesExhausted { attempts, line },
        WriteOutcome::FetchFailed(message) => SaveOutcome::FetchFailed(message),
        WriteOutcome::NoTip => SaveOutcome::NoTip,
        WriteOutcome::PushTimedOut(message) => SaveOutcome::PushTimedOut(message),
        WriteOutcome::PushFailed(stderr) => SaveOutcome::PushFailed(stderr),
    })
}

/// Edit or create one page: upsert its body under the stored front matter.
struct Edit<'a> {
    request: &'a SaveRequest,
    message: String,
}

impl WriteOp for Edit<'_> {
    type Outcome = SaveOutcome;

    async fn check_and_build(&self, at: &Fetched<'_>) -> Result<Check<SaveOutcome>, StoreError> {
        let request = self.request;
        // Step 4.
        let current = at.store.blob_at(at.tip, &request.path).await?;
        let new = match &current {
            Some((_, bytes)) => match page::check_static_rules(bytes) {
                Ok(text) => {
                    let split = page::split_front_matter(text);
                    page::compose(split.front_matter, &request.body, page::trailing_newlines(text))
                }
                Err(rule) => return Ok(Check::Refused(SaveOutcome::NotEditable(rule))),
            },
            None => page::compose("", &request.body, NEW_PAGE_TRAILING),
        };
        let current_oid = current.as_ref().map(|(oid, _)| *oid);
        let holds_new = current.as_ref().is_some_and(|(_, bytes)| *bytes == new.as_bytes());
        if current_oid != request.base_oid {
            if holds_new {
                info!("save: {} already holds the content at {}", request.path, at.tip);
                return Ok(Check::NoCommit(SaveOutcome::ContentPresent));
            }
            info!(
                "save: {} moved ({:?} -> {current_oid:?}), conflict",
                request.path, request.base_oid
            );
            let current_body = current.map(|(_, bytes)| current_body(&bytes));
            return Ok(Check::Conflict(SaveOutcome::Conflict { current_body }));
        }
        // Step 5.
        if holds_new {
            return Ok(Check::NoCommit(SaveOutcome::Unchanged));
        }
        // Step 6.
        let commit = at
            .store
            .commit_file(FileCommit {
                parent: at.tip,
                path: &request.path,
                contents: new.as_bytes(),
                author: at.author,
                committer: at.committer,
                message: &self.message,
            })
            .await?;
        Ok(Check::Committed(commit))
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

#[cfg(test)]
mod tests;
