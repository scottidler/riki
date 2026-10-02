//! The move op on the write driver (design doc, API Design op table): one commit removing `from`
//! and upserting the same blob at `to`, so git sees an exact (`R100`) rename and the redirect map
//! picks it up. A folder's `README.md` never moves (that is a folder move, parked).

use git2::Oid;
use tracing::{debug, info};

use crate::index::url_for_file;
use crate::page;
use crate::save::SaveSettings;
use crate::store::{Signer, StoreError, TreeOp};
use crate::wiki::Wiki;
use crate::write::{self, Check, Fetched, OpAnswer, WriteOp, WriteOutcome};

const DIR_PAGE: &str = "README.md";

#[derive(Debug, Clone)]
pub struct MoveRequest {
    pub from: String,
    /// The page's blob oid as the client loaded it.
    pub base_oid: Oid,
    pub to: String,
    /// The commit message; `None` uses `riki: move <from> -> <to>`.
    pub message: Option<String>,
}

/// Move `request.from` to `request.to` authored by `author`: one rename commit.
pub async fn move_page(
    wiki: &Wiki,
    settings: &SaveSettings,
    author: &Signer,
    request: &MoveRequest,
) -> Result<WriteOutcome<OpAnswer>, StoreError> {
    debug!(
        "move_page: {} -> {} base={}",
        request.from, request.to, request.base_oid
    );
    if let Err(message) = refusal(request) {
        return Ok(WriteOutcome::Op(OpAnswer::BadRequest(message)));
    }
    let message = match write::one_line_message(request.message.as_deref(), || {
        format!("riki: move {} -> {}", request.from, request.to)
    }) {
        Ok(message) => message,
        Err(err) => return Ok(WriteOutcome::Op(OpAnswer::BadRequest(err))),
    };
    write::run(wiki, settings, author, &Move { request, message }).await
}

/// The front-door guards: a request that can never succeed as sent. `Err` is the 400 message.
fn refusal(request: &MoveRequest) -> Result<(), String> {
    page::validate_page_path(&request.from).map_err(|err| err.to_string())?;
    page::validate_page_path(&request.to).map_err(|err| err.to_string())?;
    if is_dir_page(&request.from) {
        return Err(format!(
            "{} is a folder's index page; moving it is a folder move",
            request.from
        ));
    }
    if request.from == request.to {
        return Err(format!("{} is already at {}", request.from, request.to));
    }
    Ok(())
}

fn is_dir_page(file: &str) -> bool {
    file == DIR_PAGE || file.ends_with(&format!("/{DIR_PAGE}"))
}

/// The other file that serves the same URL as `file`: `a/b.md` <-> `a/b/README.md`. The root
/// `README.md` has none.
fn twin(file: &str) -> Option<String> {
    if file == DIR_PAGE {
        return None;
    }
    match file.strip_suffix(&format!("/{DIR_PAGE}")) {
        Some(dir) => Some(format!("{dir}.md")),
        None => file.strip_suffix(".md").map(|stem| format!("{stem}/{DIR_PAGE}")),
    }
}

struct Move<'a> {
    request: &'a MoveRequest,
    message: String,
}

impl Move<'_> {
    /// Why `to` cannot take the page at `at.tip` even though nothing sits at `to` itself: a file
    /// where one of its folders would go, or its URL already served by its twin.
    async fn blocked(&self, at: &Fetched<'_>) -> Result<Option<String>, StoreError> {
        let MoveRequest { from, to, .. } = self.request;
        if let Some(dir) = at.file_ancestor(to).await? {
            return Ok(Some(format!("{dir} is a file, so {to} cannot be created")));
        }
        if let Some(twin) = twin(to).filter(|twin| twin != from)
            && at.store.entry_at(at.tip, &twin).await?.is_some()
        {
            let url = url_for_file(to).unwrap_or_default();
            return Ok(Some(format!("{twin} already serves /{url}")));
        }
        Ok(None)
    }
}

impl WriteOp for Move<'_> {
    type Outcome = OpAnswer;

    async fn check_and_build(&self, at: &Fetched<'_>) -> Result<Check<OpAnswer>, StoreError> {
        let MoveRequest { from, base_oid, to, .. } = self.request;
        let from_blob = at.store.blob_at(at.tip, from).await?.map(|(oid, _)| oid);
        let to_entry = at.store.entry_at(at.tip, to).await?;
        if from_blob == Some(*base_oid) && to_entry.is_none() {
            if let Some(reason) = self.blocked(at).await? {
                info!("move: {from} -> {to} blocked at {}: {reason}", at.tip);
                return Ok(Check::Conflict(OpAnswer::Conflict(reason)));
            }
            let ops = [
                TreeOp::Remove { path: from.clone() },
                TreeOp::Upsert {
                    path: to.clone(),
                    blob: *base_oid,
                },
            ];
            let commit = at
                .store
                .commit_tree(&ops, at.tip, at.author, at.committer, &self.message)
                .await?;
            return Ok(Check::Committed(commit));
        }
        let from_absent = at.store.entry_at(at.tip, from).await?.is_none();
        let to_blob = at.store.blob_at(at.tip, to).await?.map(|(oid, _)| oid);
        if from_absent && to_blob == Some(*base_oid) {
            info!("move: {from} -> {to} already done at {}", at.tip);
            return Ok(Check::NoCommit(OpAnswer::ContentPresent));
        }
        let reason = if to_entry.is_some() {
            format!("{to} already exists")
        } else if from_absent {
            format!("{from} no longer exists")
        } else {
            format!("{from} changed since it was loaded")
        };
        info!("move: {from} -> {to} conflict at {}: {reason}", at.tip);
        Ok(Check::Conflict(OpAnswer::Conflict(reason)))
    }
}

#[cfg(test)]
mod tests;
