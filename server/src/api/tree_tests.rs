//! `GET /_riki/api/tree` and `GET /_riki/api/new-page` through the router.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use riki_core::runtime::Runtime;
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;

use crate::routes::{AppState, router};
use crate::testkit::Upstream;

async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = response.into_body().collect().await.expect("body").to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn app() -> Router {
    let upstream = Upstream::new();
    upstream.push(&[
        ("README.md", "# Home\n"),
        ("guide/README.md", "# The Guide\n"),
        ("guide/getting-started.md", "# Getting Started\n"),
        ("guide/deep/page.md", "---\ntitle: Front\n---\n# H1\n"),
        ("notes/setup/README.md", "# Setup\n"),
        ("img/logo.png", "not a page"),
    ]);
    let wiki = upstream.replica("r", std::time::Duration::from_secs(30)).await;
    router(AppState::new(Arc::new(Runtime::new()), wiki))
}

#[tokio::test]
async fn tree_lists_folders_holding_pages_and_every_page() {
    let (status, body) = get(&app().await, "/_riki/api/tree").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["folders"],
        json!(["", "guide", "guide/deep", "notes/setup"]),
        "no `img` (no page), no `notes` (holds only a folder)"
    );
    let pages = body["pages"].as_array().expect("pages");
    let page = |path: &str| pages.iter().find(|p| p["path"] == path).cloned();
    assert_eq!(
        page("README.md"),
        Some(json!({"path": "README.md", "url": "/", "title": "Home"}))
    );
    assert_eq!(
        page("guide/README.md"),
        Some(json!({"path": "guide/README.md", "url": "/guide", "title": "The Guide"}))
    );
    assert_eq!(page("guide/deep/page.md").expect("page")["title"], "Front");
    assert_eq!(pages.len(), 5);
}

#[tokio::test]
async fn new_page_derives_the_path_and_url() {
    let app = app().await;
    let (status, body) = get(&app, "/_riki/api/new-page?folder=guide&title=Fresh%20Page").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"path": "guide/fresh-page.md", "url": "/guide/fresh-page"}));
}

#[tokio::test]
async fn new_page_suffixes_a_taken_slug_and_a_reserved_one() {
    let app = app().await;
    let (_, body) = get(&app, "/_riki/api/new-page?folder=guide&title=Getting%20Started").await;
    assert_eq!(body["path"], "guide/getting-started-2.md");
    let (_, body) = get(&app, "/_riki/api/new-page?folder=notes&title=Setup").await;
    assert_eq!(body["path"], "notes/setup-2.md", "notes/setup/README.md takes it");
    let (_, body) = get(&app, "/_riki/api/new-page?title=Status").await;
    assert_eq!(body, json!({"path": "status-2.md", "url": "/status-2"}));
}

#[tokio::test]
async fn new_page_refuses_an_invalid_folder_and_a_missing_title() {
    let app = app().await;
    for uri in [
        "/_riki/api/new-page?folder=..%2Fx&title=T",
        "/_riki/api/new-page?folder=.git&title=T",
        "/_riki/api/new-page?folder=status&title=T",
        "/_riki/api/new-page?folder=guide",
    ] {
        let (status, _) = get(&app, uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
    }
}
