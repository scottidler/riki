//! The content routes: the page catch-all, the `.md` redirect, and the image route.

use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use riki_core::index::{RESERVED, url_for_file};
use riki_core::render::{encode_path, render_markdown};
use tracing::{debug, error, warn};

use crate::render::{self, Action, CSP_ASSET, CSP_PAGE};
use crate::routes::AppState;

/// Image types `/_riki/raw/` serves, with their content types. Anything else is a 404.
const IMAGE_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("svg", "image/svg+xml"),
];

pub async fn root(State(state): State<AppState>) -> Response {
    page_at(&state, "").await
}

pub async fn page(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    page_at(&state, &path).await
}

pub async fn raw(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    debug!("raw: path={path:?}");
    let Some(content_type) = image_content_type(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(index) = state.wiki.good() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let blob = match state.wiki.store().read_blob(index.commit(), &path).await {
        Ok(Some(blob)) => blob,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        // A path the store refuses (`..`, NUL, dot segment) is a miss, not a server fault.
        Err(riki_core::store::StoreError::Path(err)) => {
            warn!("raw: refused path {path:?}: {err}");
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(err) => {
            error!("raw: reading {path:?}: {err}");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Could not read that file.");
        }
    };
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP_ASSET)),
            (header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
        ],
        blob,
    )
        .into_response()
}

fn image_content_type(path: &str) -> Option<&'static str> {
    let (_, extension) = path.rsplit_once('.')?;
    IMAGE_TYPES
        .iter()
        .find(|(known, _)| extension.eq_ignore_ascii_case(known))
        .map(|(_, content_type)| *content_type)
}

async fn page_at(state: &AppState, raw_path: &str) -> Response {
    let path = raw_path.trim_matches('/');
    debug!("page_at: path={path:?}");
    if path.ends_with(".md") {
        return redirect_to_page(path);
    }
    let Some(index) = state.wiki.good() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "No content has been published yet.");
    };
    let banners = render::banners(state.wiki.rejected().as_ref(), state.wiki.unreachable().as_ref());
    let sidebar = render::sidebar(index.tree(), path);
    let Some(file) = index.file_for_url(path) else {
        return missing(path, &banners, &sidebar);
    };
    let source = match state.wiki.store().read_blob(index.commit(), file).await {
        Ok(Some(blob)) => blob,
        Ok(None) => {
            error!("page_at: {file} is in the index of {} but not the tree", index.commit());
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "That page is indexed but missing.");
        }
        Err(err) => {
            error!("page_at: reading {file}: {err}");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Could not read that page.");
        }
    };
    let body = render_markdown(file, &String::from_utf8_lossy(&source));
    let source_url = state
        .github_blob_base
        .as_deref()
        .map(|base| format!("{base}{}", encode_path(file)));
    let action = Action::Edit {
        file,
        source: source_url.as_deref(),
    };
    html_response(
        StatusCode::OK,
        render::page(&title(path), &body, &sidebar, &banners, action),
    )
}

/// `/a/b.md` -> 301 `/a/b`; `/a/README.md` -> `/a`.
fn redirect_to_page(file: &str) -> Response {
    let url = url_for_file(file).unwrap_or_default();
    let location = format!("/{}", encode_path(&url));
    (StatusCode::MOVED_PERMANENTLY, [(header::LOCATION, location)]).into_response()
}

/// A 404. Paths riki's own routes own never offer "Create this page".
fn missing(path: &str, banners: &str, sidebar: &str) -> Response {
    let top = path.split('/').next().unwrap_or_default();
    let file = format!("{path}.md");
    let creatable = !RESERVED.contains(&top) && riki_core::path::validate(&file).is_ok();
    let (body, action) = if creatable {
        (
            "<h1>Page not found</h1>\n<p>This page does not exist yet.</p>".to_string(),
            Action::Create { file: &file },
        )
    } else {
        ("<h1>Not found</h1>".to_string(), Action::None)
    };
    html_response(
        StatusCode::NOT_FOUND,
        render::page("Not found", &body, sidebar, banners, action),
    )
}

fn title(path: &str) -> String {
    match path.rsplit('/').next() {
        Some(last) if !last.is_empty() => last.to_string(),
        _ => "Home".to_string(),
    }
}

fn html_response(status: StatusCode, html: String) -> Response {
    (
        status,
        [(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP_PAGE))],
        Html(html),
    )
        .into_response()
}

fn error_response(status: StatusCode, message: &str) -> Response {
    let label = status.canonical_reason().map_or_else(
        || status.as_str().to_string(),
        |reason| format!("{} {reason}", status.as_u16()),
    );
    (status, Html(render::error_page(&label, message))).into_response()
}

#[cfg(test)]
mod tests;
