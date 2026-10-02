//! `GET /_riki/api/search` through the router.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use riki_core::runtime::Runtime;
use riki_core::wiki::Wiki;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
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

async fn served(upstream: &Upstream) -> (Router, Arc<Wiki>) {
    let wiki = upstream.replica("r", Duration::from_secs(30)).await;
    (router(AppState::new(Arc::new(Runtime::new()), wiki.clone())), wiki)
}

#[tokio::test]
async fn a_prefix_finds_the_page_and_a_poll_refreshes_the_hit() {
    let upstream = Upstream::new();
    upstream.push(&[
        ("README.md", "# Home\n\nWelcome.\n"),
        (
            "reference/tables.md",
            "# Reference\n\n## Tables\n\nPipes and dashes make a grid.\n",
        ),
    ]);
    let (app, wiki) = served(&upstream).await;
    let (status, body) = get(&app, "/_riki/api/search?q=tabl").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({"hits": [{
            "path": "reference/tables.md",
            "url": "/reference/tables",
            "title": "Reference",
            "heading": "Tables",
            "anchor": "tables",
            "snippet": "Pipes and dashes make a grid.",
            "marks": [],
        }]})
    );

    upstream.push(&[(
        "reference/tables.md",
        "# Reference\n\n## Tables\n\nA table needs a header row.\n",
    )]);
    wiki.poll().await.expect("poll");
    let (_, body) = get(&app, "/_riki/api/search?q=tabl").await;
    let hit = &body["hits"][0];
    assert_eq!(hit["snippet"], "A table needs a header row.");
    assert_eq!(hit["marks"], json!([[2, 6]]), "the body now matches too");
    let (_, body) = get(&app, "/_riki/api/search?q=pipes").await;
    assert_eq!(body, json!({"hits": []}), "the old text is gone after the poll");
}

#[tokio::test]
async fn the_home_page_hit_has_url_slash_and_an_unheaded_section_has_no_anchor() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "Laptop push check: this line came from a git push\n")]);
    let (app, _) = served(&upstream).await;
    let (_, body) = get(&app, "/_riki/api/search?q=lapt").await;
    let hit = &body["hits"][0];
    assert_eq!(hit["url"], "/");
    assert_eq!(hit["title"], "Home");
    assert_eq!(hit["heading"], Value::Null);
    assert_eq!(hit["anchor"], Value::Null);
}

#[tokio::test]
async fn limit_defaults_to_20_and_is_capped_at_50() {
    let upstream = Upstream::new();
    let pages: Vec<(String, String)> = (0..60)
        .map(|n| (format!("p{n:02}.md"), format!("# Page {n}\n\nzebra\n")))
        .collect();
    let mut files: Vec<(&str, &str)> = pages.iter().map(|(p, c)| (p.as_str(), c.as_str())).collect();
    files.push(("README.md", "# Home\n"));
    upstream.push(&files);
    let (app, _) = served(&upstream).await;
    let count = |body: &Value| body["hits"].as_array().expect("hits").len();
    let (_, body) = get(&app, "/_riki/api/search?q=zebra").await;
    assert_eq!(count(&body), 20);
    let (_, body) = get(&app, "/_riki/api/search?q=zebra&limit=5").await;
    assert_eq!(count(&body), 5);
    assert_eq!(body["hits"][0]["path"], "p00.md", "ties by path");
    let (_, body) = get(&app, "/_riki/api/search?q=zebra&limit=500").await;
    assert_eq!(count(&body), 50);
}

#[tokio::test]
async fn a_missing_query_or_a_bad_limit_is_400_and_nothing_published_is_503() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# Home\n")]);
    let (app, _) = served(&upstream).await;
    for uri in [
        "/_riki/api/search",
        "/_riki/api/search?q=x&limit=many",
        "/_riki/api/search?q=x&limit=-1",
    ] {
        let (status, _) = get(&app, uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
    }
    let (status, body) = get(&app, "/_riki/api/search?q=").await;
    assert_eq!((status, body), (StatusCode::OK, json!({"hits": []})));

    let empty = Upstream::new();
    let wiki = Arc::new(Wiki::open(&empty.store_config()).await.expect("open"));
    let app = router(AppState::new(Arc::new(Runtime::new()), wiki));
    let (status, _) = get(&app, "/_riki/api/search?q=x").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}
