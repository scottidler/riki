//! The delete and restore ops on the write driver (design doc, API Design op table). Delete is one
//! `Remove` commit. Restore re-upserts the blob a delete commit D removed, read from D's parent
//! tree, with git's revert message; it never calls libgit2's revert (Resolved Decisions).

use git2::Oid;
use tracing::{debug, info};

use crate::page;
use crate::save::SaveSettings;
use crate::store::{Signer, StoreError, TreeOp};
use crate::wiki::Wiki;
use crate::write::{self, Check, Fetched, OpAnswer, WriteOp, WriteOutcome};

/// The site's home page; deleting it is refused.
pub const ROOT_README: &str = "README.md";

#[derive(Debug, Clone)]
pub struct DeleteRequest {
    pub path: String,
    /// The page's blob oid as the client loaded it.
    pub base_oid: Oid,
    /// The commit message; `None` uses `riki: delete <path>`.
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RestoreRequest {
    pub path: String,
    /// D: the delete commit whose removal of `path` is undone.
    pub commit: Oid,
}

/// Delete `request.path` authored by `author`: one commit removing it.
pub async fn delete(
    wiki: &Wiki,
    settings: &SaveSettings,
    author: &Signer,
    request: &DeleteRequest,
) -> Result<WriteOutcome<OpAnswer>, StoreError> {
    debug!("delete: path={} base={}", request.path, request.base_oid);
    if let Err(err) = page::validate_page_path(&request.path) {
        return Ok(WriteOutcome::Op(OpAnswer::BadRequest(err.to_string())));
    }
    if request.path == ROOT_README {
        return Ok(WriteOutcome::Op(OpAnswer::BadRequest(
            "the root README.md is the home page and cannot be deleted".to_string(),
        )));
    }
    let message = match write::one_line_message(request.message.as_deref(), || format!("riki: delete {}", request.path))
    {
        Ok(message) => message,
        Err(err) => return Ok(WriteOutcome::Op(OpAnswer::BadRequest(err))),
    };
    write::run(wiki, settings, author, &Delete { request, message }).await
}

/// Restore `request.path` as delete commit `request.commit` removed it, authored by `author`.
pub async fn restore(
    wiki: &Wiki,
    settings: &SaveSettings,
    author: &Signer,
    request: &RestoreRequest,
) -> Result<WriteOutcome<OpAnswer>, StoreError> {
    debug!("restore: path={} commit={}", request.path, request.commit);
    if let Err(err) = page::validate_page_path(&request.path) {
        return Ok(WriteOutcome::Op(OpAnswer::BadRequest(err.to_string())));
    }
    write::run(wiki, settings, author, &Restore { request }).await
}

struct Delete<'a> {
    request: &'a DeleteRequest,
    message: String,
}

impl WriteOp for Delete<'_> {
    type Outcome = OpAnswer;

    async fn check_and_build(&self, at: &Fetched<'_>) -> Result<Check<OpAnswer>, StoreError> {
        let path = &self.request.path;
        // Oids hash the object type, so an entry with the base blob's oid is that blob.
        match at.store.entry_at(at.tip, path).await? {
            None => {
                info!("delete: {path} already absent at {}", at.tip);
                Ok(Check::NoCommit(OpAnswer::ContentPresent))
            }
            Some(oid) if oid == self.request.base_oid => {
                let ops = [TreeOp::Remove { path: path.clone() }];
                let commit = at
                    .store
                    .commit_tree(&ops, at.tip, at.author, at.committer, &self.message)
                    .await?;
                Ok(Check::Committed(commit))
            }
            Some(oid) => {
                info!("delete: {path} moved ({} -> {oid}), conflict", self.request.base_oid);
                Ok(Check::Conflict(OpAnswer::Conflict(format!(
                    "{path} changed since it was loaded"
                ))))
            }
        }
    }
}

struct Restore<'a> {
    request: &'a RestoreRequest,
}

/// A recognized delete commit D: what it removed at the path, and its subject for the message.
struct Deleted {
    blob: Oid,
    summary: String,
}

impl Restore<'_> {
    /// D recognition (design doc, Data Model): D is the tip or an ancestor of it, has exactly one
    /// parent, and the path is a file in that parent and absent in D. `Err` is the 400 message.
    async fn recognize(&self, at: &Fetched<'_>) -> Result<Result<Deleted, String>, StoreError> {
        let RestoreRequest { path, commit } = self.request;
        let Some(info) = at.store.commit_info(*commit).await? else {
            return Ok(Err(format!("{commit} is not a commit")));
        };
        if !at.store.reaches(at.tip, *commit).await? {
            return Ok(Err(format!("{commit} is not in the branch history")));
        }
        let [parent] = info.parents[..] else {
            return Ok(Err(format!(
                "{commit} has {} parents, a delete has one",
                info.parents.len()
            )));
        };
        let Some((blob, _)) = at.store.blob_at(parent, path).await? else {
            return Ok(Err(format!("{path} is not a file before {commit}")));
        };
        if at.store.entry_at(*commit, path).await?.is_some() {
            return Ok(Err(format!("{commit} did not remove {path}")));
        }
        Ok(Ok(Deleted {
            blob,
            summary: info.summary,
        }))
    }
}

impl WriteOp for Restore<'_> {
    type Outcome = OpAnswer;

    async fn check_and_build(&self, at: &Fetched<'_>) -> Result<Check<OpAnswer>, StoreError> {
        let RestoreRequest { path, commit } = self.request;
        let deleted = match self.recognize(at).await? {
            Ok(deleted) => deleted,
            Err(message) => {
                info!("restore: refused {commit} for {path}: {message}");
                return Ok(Check::Refused(OpAnswer::BadRequest(message)));
            }
        };
        match at.store.entry_at(at.tip, path).await? {
            None => {
                if let Some(dir) = at.file_ancestor(path).await? {
                    info!("restore: {dir} is a file at {}, conflict", at.tip);
                    return Ok(Check::Conflict(OpAnswer::Conflict(format!(
                        "{dir} is a file, so {path} cannot be restored"
                    ))));
                }
                let ops = [TreeOp::Upsert {
                    path: path.clone(),
                    blob: deleted.blob,
                }];
                let message = revert_message(&deleted.summary, *commit);
                let commit = at
                    .store
                    .commit_tree(&ops, at.tip, at.author, at.committer, &message)
                    .await?;
                Ok(Check::Committed(commit))
            }
            Some(oid) if oid == deleted.blob => {
                info!("restore: {path} already holds the deleted bytes at {}", at.tip);
                Ok(Check::NoCommit(OpAnswer::ContentPresent))
            }
            Some(oid) => {
                info!("restore: {path} re-created as {oid}, conflict");
                Ok(Check::Conflict(OpAnswer::Conflict(format!(
                    "{path} was re-created with different contents"
                ))))
            }
        }
    }
}

/// git's own revert message for `commit` with subject `summary`.
fn revert_message(summary: &str, commit: Oid) -> String {
    format!("Revert \"{summary}\"\n\nThis reverts commit {commit}.")
}

#[cfg(test)]
mod tests;
