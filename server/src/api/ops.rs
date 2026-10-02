//! The path ops: `POST /_riki/api/delete` and `POST /_riki/api/restore`. The ops themselves are
//! `riki_core::delete`; this file parses the request and maps the outcome to HTTP, with the same
//! status mapping as save.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use riki_core::Oid;
use riki_core::delete::{self, DeleteRequest, RestoreRequest};
use riki_core::index::url_for_file;
use riki_core::store::Signer;
use riki_core::write::{OpAnswer, WriteOutcome};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::{error, fetch_failed, index_conflict, internal, no_tip, parse_oid, push_timed_out, retries_exhausted};
use crate::identity::Identity;
use crate::routes::AppState;

pub(super) const DELETE: &str = "/_riki/api/delete";
pub(super) const RESTORE: &str = "/_riki/api/restore";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct DeleteBody {
    path: String,
    base_oid: String,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(super) struct RestoreBody {
    path: String,
    commit: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct OpBody {
    /// `None` when the op made no commit (`content-present: true`).
    commit: Option<String>,
    content_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

fn author(identity: Identity) -> Signer {
    Signer {
        name: identity.name,
        email: identity.email,
    }
}

/// Delete a page: one commit removing it. 200 `{commit}`; `commit: null` with
/// `content-present: true` when the page is already gone.
pub(super) async fn delete(
    State(state): State<AppState>,
    identity: Identity,
    body: Result<Json<DeleteBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => return error(StatusCode::BAD_REQUEST, rejection.body_text()),
    };
    let base_oid = match parse_oid("base-oid", &body.base_oid) {
        Ok(oid) => oid,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let request = DeleteRequest {
        path: body.path,
        base_oid,
        message: body.message,
    };
    let author = author(identity);
    let outcome = match delete::delete(&state.wiki, &state.save, &author, &request).await {
        Ok(outcome) => outcome,
        Err(err) => return internal("deleting", err),
    };
    info!("delete: {} by {} -> {outcome:?}", request.path, author.email);
    op_response("delete", outcome, None)
}

/// Undo a delete: re-add the bytes delete commit `commit` removed at `path`. 200 `{commit, url}`.
pub(super) async fn restore(
    State(state): State<AppState>,
    identity: Identity,
    body: Result<Json<RestoreBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => return error(StatusCode::BAD_REQUEST, rejection.body_text()),
    };
    let commit = match parse_oid("commit", &body.commit) {
        Ok(oid) => oid,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let request = RestoreRequest {
        path: body.path,
        commit,
    };
    let author = author(identity);
    let outcome = match delete::restore(&state.wiki, &state.save, &author, &request).await {
        Ok(outcome) => outcome,
        Err(err) => return internal("restoring", err),
    };
    info!("restore: {} by {} -> {outcome:?}", request.path, author.email);
    let url = url_for_file(&request.path).map(|url| format!("/{url}"));
    op_response("restore", outcome, url)
}

/// One `match` from a path op's outcome to HTTP. `url` rides on every 200.
fn op_response(op: &str, outcome: WriteOutcome<OpAnswer>, url: Option<String>) -> Response {
    let ok = |commit: Option<Oid>, content_present| {
        let body = OpBody {
            commit: commit.map(|oid| oid.to_string()),
            content_present,
            url,
        };
        (StatusCode::OK, Json(body)).into_response()
    };
    match outcome {
        WriteOutcome::Pushed { commit } => ok(Some(commit), false),
        WriteOutcome::Op(OpAnswer::ContentPresent) => ok(None, true),
        WriteOutcome::Op(OpAnswer::Conflict(message)) => error(StatusCode::CONFLICT, message),
        WriteOutcome::Op(OpAnswer::BadRequest(message)) => error(StatusCode::BAD_REQUEST, message),
        WriteOutcome::IndexConflict { errors } => index_conflict(op, &errors),
        WriteOutcome::RetriesExhausted { attempts, line } => retries_exhausted(attempts, &line),
        WriteOutcome::FetchFailed(message) => fetch_failed(&message),
        WriteOutcome::NoTip => no_tip(),
        WriteOutcome::PushTimedOut(message) => push_timed_out(&message),
        WriteOutcome::PushFailed(stderr) => error(StatusCode::BAD_GATEWAY, stderr),
    }
}
