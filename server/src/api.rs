//! The page API: `GET /_riki/api/page` (the editor's view of a page), `POST /_riki/api/page`
//! (save), `POST /_riki/api/roundtrip` (the round-trip guard's comparison), and the path ops
//! `POST /_riki/api/move`, `POST /_riki/api/delete`, and `POST /_riki/api/restore` (`ops`).
//!
//! Every POST route sits behind the JSON-only guard: a cross-origin JSON POST needs a CORS
//! preflight riki never answers, so requiring `application/json` blocks form-based CSRF. The save
//! algorithm itself is `riki_core::save`; this file maps its outcomes to HTTP.

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use riki_core::Oid;
use riki_core::page::{self, StaticRule};
use riki_core::save::{self, SaveOutcome, SaveRequest};
use riki_core::store::Signer;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

use crate::identity::Identity;
use crate::routes::AppState;

const PAGE: &str = "/_riki/api/page";
const ROUNDTRIP: &str = "/_riki/api/roundtrip";

pub fn router() -> Router<AppState> {
    let posts = Router::new()
        .route(PAGE, post(save))
        .route(ROUNDTRIP, post(roundtrip))
        .route(ops::MOVE, post(ops::move_page))
        .route(ops::DELETE, post(ops::delete))
        .route(ops::RESTORE, post(ops::restore))
        .route_layer(middleware::from_fn(require_json));
    Router::new().route(PAGE, get(load)).merge(posts)
}

/// 415 unless `Content-Type` is `application/json`; parameters (`; charset=utf-8`) are accepted.
async fn require_json(request: Request, next: Next) -> Response {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());
    if !is_json(content_type) {
        debug!("require_json: refused content-type {content_type:?}");
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Content-Type must be application/json\n",
        )
            .into_response();
    }
    next.run(request).await
}

fn is_json(content_type: Option<&str>) -> bool {
    content_type
        .and_then(|value| value.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct ErrorBody {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_safe: Option<bool>,
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    let body = ErrorBody {
        error: message.into(),
        retry_safe: None,
    };
    (status, Json(body)).into_response()
}

fn internal(context: &str, err: impl std::fmt::Display) -> Response {
    error!("{context}: {err}");
    error(StatusCode::INTERNAL_SERVER_ERROR, format!("{context}: {err}"))
}

/// Parse the oid in request field `field`; the error is the 400 message.
fn parse_oid(field: &str, text: &str) -> Result<Oid, String> {
    text.parse::<Oid>()
        .map_err(|_| format!("{field} {text:?} is not an object id"))
}

#[derive(Debug, Deserialize)]
struct LoadQuery {
    path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct LoadBody {
    path: String,
    base_oid: Option<String>,
    /// The body without front matter; `None` when the page is not editable.
    body: Option<String>,
    editable: bool,
    reason: Option<String>,
}

/// The page as the editor loads it, from the good tip. Front matter is never sent; a file
/// breaking a static rule is `editable: false` with the rule as the reason.
async fn load(State(state): State<AppState>, query: Result<Query<LoadQuery>, QueryRejection>) -> Response {
    let Ok(Query(LoadQuery { path })) = query else {
        return error(StatusCode::BAD_REQUEST, "missing `path` query parameter");
    };
    debug!("load: path={path:?}");
    if let Err(err) = page::validate_page_path(&path) {
        return error(StatusCode::BAD_REQUEST, err.to_string());
    }
    let blob = match state.wiki.good() {
        Some(index) => match state.wiki.store().blob_at(index.commit(), &path).await {
            Ok(blob) => blob,
            Err(err) => return internal("reading the page", err),
        },
        None => None,
    };
    let body = match blob {
        None => LoadBody {
            path,
            base_oid: None,
            body: Some(String::new()),
            editable: true,
            reason: None,
        },
        Some((oid, bytes)) => match page::check_static_rules(&bytes) {
            Ok(text) => LoadBody {
                path,
                base_oid: Some(oid.to_string()),
                body: Some(page::split_front_matter(text).body.to_string()),
                editable: true,
                reason: None,
            },
            Err(rule) => LoadBody {
                path,
                base_oid: Some(oid.to_string()),
                body: None,
                editable: false,
                reason: Some(rule.to_string()),
            },
        },
    };
    Json(body).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct SaveBody {
    path: String,
    base_oid: Option<String>,
    body: String,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct SavedBody {
    commit: Option<String>,
    content_present: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct ConflictBody {
    error: String,
    current_body: Option<String>,
}

/// Save (design doc, save algorithm). Step 1's guards are the JSON route layer and the
/// `Identity` extractor; everything after is `riki_core::save`.
async fn save(
    State(state): State<AppState>,
    identity: Identity,
    body: Result<Json<SaveBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => return error(StatusCode::BAD_REQUEST, rejection.body_text()),
    };
    let base_oid = match body
        .base_oid
        .as_deref()
        .map(|oid| parse_oid("base-oid", oid))
        .transpose()
    {
        Ok(oid) => oid,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let request = SaveRequest {
        path: body.path,
        base_oid,
        body: body.body,
        message: body.message,
    };
    let author = Signer {
        name: identity.name,
        email: identity.email,
    };
    let outcome = match save::save(&state.wiki, &state.save, &author, &request).await {
        Ok(outcome) => outcome,
        Err(err) => return internal("saving", err),
    };
    info!("save: {} by {} -> {outcome:?}", request.path, author.email);
    save_response(outcome)
}

fn save_response(outcome: SaveOutcome) -> Response {
    let saved = |commit: Option<Oid>, content_present| {
        let body = SavedBody {
            commit: commit.map(|oid| oid.to_string()),
            content_present,
        };
        (StatusCode::OK, Json(body)).into_response()
    };
    match outcome {
        SaveOutcome::Saved { commit } => saved(Some(commit), false),
        SaveOutcome::Unchanged => saved(None, false),
        SaveOutcome::ContentPresent => saved(None, true),
        SaveOutcome::Conflict { current_body } => {
            let body = ConflictBody {
                error: "the page changed since it was loaded".to_string(),
                current_body,
            };
            (StatusCode::CONFLICT, Json(body)).into_response()
        }
        SaveOutcome::IndexConflict { errors } => index_conflict("save", &errors),
        SaveOutcome::RetriesExhausted { attempts, line } => retries_exhausted(attempts, &line),
        SaveOutcome::BadRequest(message) => error(StatusCode::BAD_REQUEST, message),
        SaveOutcome::NotEditable(rule) => not_editable(rule),
        SaveOutcome::FetchFailed(message) => fetch_failed(&message),
        SaveOutcome::NoTip => no_tip(),
        SaveOutcome::PushTimedOut(message) => push_timed_out(&message),
        SaveOutcome::PushFailed(stderr) => error(StatusCode::BAD_GATEWAY, stderr),
    }
}

// The write driver's endings, shared by save and every path op (design doc, API Design status
// mapping): 409 for the index and retry conflicts, plain 503 for fetch failures, 503
// `retry-safe` only when a push timed out.

fn index_conflict(op: &str, errors: &str) -> Response {
    error(StatusCode::CONFLICT, format!("the {op} would break the wiki: {errors}"))
}

fn retries_exhausted(attempts: u32, line: &str) -> Response {
    error(
        StatusCode::CONFLICT,
        format!("upstream kept moving: push rejected {attempts} times ({line})"),
    )
}

fn fetch_failed(message: &str) -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        format!("upstream unreachable: {message}"),
    )
}

fn no_tip() -> Response {
    error(StatusCode::SERVICE_UNAVAILABLE, "upstream has no commit on the branch")
}

fn push_timed_out(message: &str) -> Response {
    let body = ErrorBody {
        error: format!("push outcome unknown: {message}"),
        retry_safe: Some(true),
    };
    (StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response()
}

fn not_editable(rule: StaticRule) -> Response {
    error(
        StatusCode::UNPROCESSABLE_ENTITY,
        format!("this page cannot be edited in the browser: {rule}"),
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RoundtripBody {
    path: String,
    base_oid: String,
    serialized: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct RoundtripResult {
    identical: bool,
    first_diff_line: Option<usize>,
}

/// The round-trip guard: compare the editor's no-edit serialization with the body of blob
/// `base-oid`, trailing newlines aside. Every result is logged at INFO.
async fn roundtrip(State(state): State<AppState>, body: Result<Json<RoundtripBody>, JsonRejection>) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => return error(StatusCode::BAD_REQUEST, rejection.body_text()),
    };
    let oid = match parse_oid("base-oid", &body.base_oid) {
        Ok(oid) => oid,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let bytes = match state.wiki.store().blob(oid).await {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return error(StatusCode::NOT_FOUND, format!("no blob {oid}")),
        Err(err) => return internal("reading the blob", err),
    };
    let text = match page::check_static_rules(&bytes) {
        Ok(text) => text,
        Err(rule) => return not_editable(rule),
    };
    let first_diff_line = page::first_diff_line(page::split_front_matter(text).body, &body.serialized);
    info!(
        "roundtrip: path={} base-oid={oid} identical={} first-diff-line={first_diff_line:?}",
        body.path,
        first_diff_line.is_none()
    );
    Json(RoundtripResult {
        identical: first_diff_line.is_none(),
        first_diff_line,
    })
    .into_response()
}

mod ops;

#[cfg(test)]
mod ops_tests;
#[cfg(test)]
mod save_tests;
#[cfg(test)]
mod tests;
