//! The content routes: the page catch-all, the `.md` redirect, the moved-page redirects, and the
//! image route.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use riki_core::index::{PageNode, RESERVED, label, prettify, url_for_file};
use riki_core::render::{encode_path, render_markdown};
use serde::Deserialize;
use tracing::{debug, error, warn};

use crate::render::{self, Action, CSP_ASSET, CSP_PAGE, PageView};
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

/// A page request's query. `new` (the new-page title) marks an explicit create: a missing page
/// renders "Create this page" instead of following a redirect away from the URL being created.
#[derive(Debug, Default, Deserialize)]
pub struct PageQuery {
    new: Option<String>,
}

pub async fn root(State(state): State<AppState>, Query(query): Query<PageQuery>) -> Response {
    page_at(&state, "", &query).await
}

pub async fn page(State(state): State<AppState>, Path(path): Path<String>, Query(query): Query<PageQuery>) -> Response {
    page_at(&state, &path, &query).await
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

/// The content type `/_riki/raw/` serves `path` with, by extension; `None` when it serves no such
/// file type.
pub fn image_content_type(path: &str) -> Option<&'static str> {
    let (_, extension) = path.rsplit_once('.')?;
    IMAGE_TYPES
        .iter()
        .find(|(known, _)| extension.eq_ignore_ascii_case(known))
        .map(|(_, content_type)| *content_type)
}

async fn page_at(state: &AppState, raw_path: &str, query: &PageQuery) -> Response {
    let path = raw_path.trim_matches('/');
    debug!("page_at: path={path:?} new={:?}", query.new);
    if path.ends_with(".md") {
        return redirect_to_page(path);
    }
    let Some(published) = state.wiki.good() else {
        return error_response(StatusCode::SERVICE_UNAVAILABLE, "No content has been published yet.");
    };
    let index = &published.nav;
    if index.file_for_url(path).is_none()
        && query.new.is_none()
        && let Some(target) = published
            .redirects
            .resolve(path, |url| index.file_for_url(url).is_some())
    {
        debug!("page_at: /{path} moved to /{target}");
        return moved_to(target);
    }
    let banners = render::banners(state.wiki.rejected().as_ref(), state.wiki.unreachable().as_ref());
    let tree = index.tree();
    let sidebar = render::sidebar(tree, path);
    let chrome = Chrome {
        tree,
        path,
        site: &state.site,
        sidebar: &sidebar,
        banners: &banners,
    };
    let Some(file) = index.file_for_url(path) else {
        return missing(&chrome);
    };
    let (base_oid, source) = match state.wiki.store().blob_at(index.commit(), file).await {
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
    let rendered = render_markdown(file, &String::from_utf8_lossy(&source));
    let source_url = state
        .github_blob_base
        .as_deref()
        .map(|base| format!("{base}{}", encode_path(file)));
    let action = Action::Edit {
        file,
        base_oid: &base_oid.to_string(),
        source: source_url.as_deref(),
    };
    let title = index.node(path).map_or_else(
        || last_segment(path),
        |node| label(node, path.rsplit('/').next().unwrap_or(path)),
    );
    html_response(
        StatusCode::OK,
        render::page(&PageView {
            title: &title,
            site: chrome.site,
            body_html: &rendered.html,
            toc: &rendered.toc,
            sidebar_html: chrome.sidebar,
            breadcrumbs_html: &render::breadcrumbs(tree, path, &title),
            trail_html: &render::trail(tree, path, &title),
            pager_html: &render::pager(tree, path),
            banners_html: chrome.banners,
            action,
        }),
    )
}

/// The parts of a page every response at `path` shares.
struct Chrome<'a> {
    tree: &'a PageNode,
    path: &'a str,
    site: &'a render::Site,
    sidebar: &'a str,
    banners: &'a str,
}

/// `/a/b.md` -> 301 `/a/b`; `/a/README.md` -> `/a`.
fn redirect_to_page(file: &str) -> Response {
    moved_to(&url_for_file(file).unwrap_or_default())
}

/// 301 to the page at `url` (no leading `/`).
fn moved_to(url: &str) -> Response {
    let location = format!("/{}", encode_path(url));
    (StatusCode::MOVED_PERMANENTLY, [(header::LOCATION, location)]).into_response()
}

/// A 404. Paths riki's own routes own never offer "Create this page". The homepage `/` creates
/// `README.md`; any other page `/a/b` creates `a/b.md`.
fn missing(chrome: &Chrome<'_>) -> Response {
    let path = chrome.path;
    let top = path.split('/').next().unwrap_or_default();
    let file = new_page_file(path);
    let creatable = !RESERVED.contains(&top) && riki_core::path::validate(&file).is_ok();
    let (body, action) = if creatable {
        (
            "<h1>Page not found</h1>\n<p>This page does not exist yet.</p>".to_string(),
            Action::Create { file: &file },
        )
    } else {
        ("<h1>Not found</h1>".to_string(), Action::None)
    };
    let title = last_segment(path);
    html_response(
        StatusCode::NOT_FOUND,
        render::page(&PageView {
            title: "Not found",
            site: chrome.site,
            body_html: &body,
            toc: &[],
            sidebar_html: chrome.sidebar,
            breadcrumbs_html: &render::breadcrumbs(chrome.tree, path, &title),
            trail_html: &render::trail(chrome.tree, path, &title),
            pager_html: "",
            banners_html: chrome.banners,
            action,
        }),
    )
}

/// The file "Create this page" makes for the URL `path` (already trimmed of slashes).
fn new_page_file(path: &str) -> String {
    if path.is_empty() {
        "README.md".to_string()
    } else {
        format!("{path}.md")
    }
}

/// The title of a path with no page node: its last URL segment, prettified (`Home` for the root).
fn last_segment(path: &str) -> String {
    match path.rsplit('/').next() {
        Some(last) if !last.is_empty() => prettify(last),
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
