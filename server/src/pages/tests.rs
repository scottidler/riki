use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use riki_core::runtime::Runtime;
use riki_core::wiki::Wiki;
use tower::ServiceExt;

use crate::routes::{AppState, router};
use crate::testkit::Upstream;

struct Fx {
    upstream: Upstream,
    wiki: Arc<Wiki>,
}

async fn wiki_with(files: &[(&str, &str)]) -> Fx {
    let upstream = Upstream::new();
    upstream.push(files);
    let wiki = upstream.wiki().await;
    wiki.poll().await.expect("poll");
    Fx { upstream, wiki }
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    fn header(&self, name: header::HeaderName) -> &str {
        self.headers.get(name).expect("header").to_str().expect("ascii")
    }
}

async fn get(wiki: &Arc<Wiki>, path: &str) -> Reply {
    send(AppState::new(Arc::new(Runtime::new()), wiki.clone()), path).await
}

async fn send(state: AppState, path: &str) -> Reply {
    let app = router(state);
    let response = app
        .oneshot(Request::get(path).body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let (parts, body) = response.into_parts();
    Reply {
        status: parts.status,
        headers: parts.headers,
        body: body.collect().await.expect("body").to_bytes().to_vec(),
    }
}

#[tokio::test]
async fn root_serves_the_readme() {
    let fx = wiki_with(&[("README.md", "# Welcome home\n")]).await;
    let reply = get(&fx.wiki, "/").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.text().contains("Welcome home"), "{}", reply.text());
    assert_eq!(reply.header(header::CONTENT_SECURITY_POLICY), crate::render::CSP_PAGE);
    assert!(
        reply
            .header(header::CONTENT_SECURITY_POLICY)
            .contains("script-src 'self'")
    );
}

#[tokio::test]
async fn nested_readme_resolves_links_and_images_against_its_own_directory() {
    let fx = wiki_with(&[
        ("README.md", "# home\n"),
        ("a/b/README.md", "[x](../x.md) ![pic](img.png)\n"),
        ("a/x.md", "# x\n"),
    ])
    .await;
    let reply = get(&fx.wiki, "/a/b").await;
    assert_eq!(reply.status, StatusCode::OK);
    let html = reply.text();
    assert!(html.contains(r#"href="/a/x""#), "{html}");
    assert!(html.contains(r#"src="/_riki/raw/a/b/img.png""#), "{html}");
    assert_eq!(get(&fx.wiki, "/a/x").await.status, StatusCode::OK);
}

#[tokio::test]
async fn md_urls_redirect_permanently_to_the_page() {
    let fx = wiki_with(&[("README.md", "# home\n"), ("a/b.md", "b\n"), ("c/README.md", "c\n")]).await;
    let reply = get(&fx.wiki, "/a/b.md").await;
    assert_eq!(reply.status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(reply.header(header::LOCATION), "/a/b");
    let reply = get(&fx.wiki, "/c/README.md").await;
    assert_eq!(reply.status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(reply.header(header::LOCATION), "/c");
    let reply = get(&fx.wiki, "/README.md").await;
    assert_eq!(reply.header(header::LOCATION), "/");
}

#[tokio::test]
async fn pages_show_the_sidebar_and_an_edit_button() {
    let fx = wiki_with(&[("README.md", "# home\n"), ("a/b.md", "b\n")]).await;
    let html = get(&fx.wiki, "/a/b").await.text();
    assert!(html.contains(r#"<a href="/a/b" class="current">b</a>"#), "{html}");
    assert!(html.contains("id=\"riki-edit\""), "{html}");
}

#[tokio::test]
async fn the_edit_button_names_the_served_file() {
    let fx = wiki_with(&[("README.md", "# home\n"), ("a/b/README.md", "b\n")]).await;
    let html = get(&fx.wiki, "/a/b").await.text();
    assert!(
        html.contains(r#"<button id="riki-edit" type="button" data-path="a/b/README.md">Edit</button>"#),
        "{html}"
    );
}

#[tokio::test]
async fn the_edit_button_links_the_file_on_github_when_configured() {
    let fx = wiki_with(&[("README.md", "# home\n"), ("a b.md", "x\n")]).await;
    let state = AppState::new(Arc::new(Runtime::new()), fx.wiki.clone())
        .with_github_blob_base(Some("https://github.com/o/r/blob/main/".to_string()));
    let html = send(state, "/a%20b").await.text();
    assert!(
        html.contains(r#"data-source="https://github.com/o/r/blob/main/a%20b.md""#),
        "{html}"
    );
}

#[tokio::test]
async fn the_editor_assets_are_served_alongside_pages() {
    let fx = wiki_with(&[("README.md", "# home\n")]).await;
    let reply = get(&fx.wiki, "/_riki/assets/editor.js").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.header(header::CONTENT_TYPE), "text/javascript; charset=utf-8");
}

#[tokio::test]
async fn a_missing_page_is_404_with_create_this_page() {
    let fx = wiki_with(&[("README.md", "# home\n")]).await;
    let reply = get(&fx.wiki, "/nope/deeper").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    let html = reply.text();
    assert!(html.contains("Create this page"), "{html}");
    assert!(html.contains(r#"data-path="nope/deeper.md""#), "{html}");
    assert!(!html.contains("riki-edit"), "{html}");
}

#[tokio::test]
async fn reserved_paths_never_offer_create() {
    let fx = wiki_with(&[("README.md", "# home\n")]).await;
    for path in ["/_riki/anything", "/health/x", "/status/x"] {
        let reply = get(&fx.wiki, path).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
        assert!(!reply.text().contains("Create this page"), "{path}");
    }
}

#[tokio::test]
async fn raw_serves_images_with_the_asset_csp() {
    let fx = wiki_with(&[("README.md", "# home\n")]).await;
    fx.upstream.push_bytes(&[
        ("a/img.png", b"\x89PNG\r\n\x1a\n\xff\xfe"),
        ("logo.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
    ]);
    fx.wiki.poll().await.expect("poll");
    let reply = get(&fx.wiki, "/_riki/raw/a/img.png").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.header(header::CONTENT_TYPE), "image/png");
    assert_eq!(
        reply.header(header::CONTENT_SECURITY_POLICY),
        "default-src 'none'; frame-ancestors 'self'"
    );
    assert_eq!(reply.body, b"\x89PNG\r\n\x1a\n\xff\xfe");
    let svg = get(&fx.wiki, "/_riki/raw/logo.svg").await;
    assert_eq!(svg.header(header::CONTENT_TYPE), "image/svg+xml");
    assert_eq!(
        svg.header(header::CONTENT_SECURITY_POLICY),
        "default-src 'none'; frame-ancestors 'self'"
    );
}

#[tokio::test]
async fn raw_404s_every_other_extension() {
    let fx = wiki_with(&[
        ("README.md", "# home\n"),
        ("x.html", "<script>1</script>"),
        ("n.txt", "hi"),
    ])
    .await;
    for path in [
        "/_riki/raw/x.html",
        "/_riki/raw/n.txt",
        "/_riki/raw/README.md",
        "/_riki/raw/noext",
    ] {
        assert_eq!(get(&fx.wiki, path).await.status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn raw_404s_missing_and_refused_paths() {
    let fx = wiki_with(&[("README.md", "# home\n")]).await;
    assert_eq!(get(&fx.wiki, "/_riki/raw/gone.png").await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        get(&fx.wiki, "/_riki/raw/a/..%2F..%2Fx.png").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&fx.wiki, "/_riki/raw/.git/x.png").await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_tip_adding_status_md_shows_the_banner_and_status_stays_json() {
    let fx = wiki_with(&[("README.md", "# home\n")]).await;
    let bad = fx.upstream.push(&[("status.md", "# not allowed\n")]);
    fx.wiki.poll().await.expect("poll");

    let html = get(&fx.wiki, "/").await.text();
    assert!(html.contains("banner error"), "{html}");
    assert!(html.contains(&bad.to_string()), "{html}");
    assert!(html.contains("status.md"), "{html}");

    let status = get(&fx.wiki, "/status").await;
    assert_eq!(status.status, StatusCode::OK);
    assert_eq!(status.header(header::CONTENT_TYPE), "application/json");
    let json: serde_json::Value = serde_json::from_slice(&status.body).expect("json");
    assert_eq!(json["status"], "degraded");
}

#[tokio::test]
async fn unreachable_upstream_shows_its_banner() {
    let fx = wiki_with(&[("README.md", "# home\n")]).await;
    let parked = fx.upstream.tmp.path().join("parked.git");
    std::fs::rename(&fx.upstream.dir, &parked).expect("park");
    fx.wiki.poll().await.expect("poll");
    let html = get(&fx.wiki, "/").await.text();
    assert!(html.contains("Upstream unreachable since"), "{html}");
    std::fs::rename(&parked, &fx.upstream.dir).expect("restore");
    fx.wiki.poll().await.expect("poll");
    assert!(!get(&fx.wiki, "/").await.text().contains("Upstream unreachable"));
}

#[tokio::test]
async fn no_published_content_is_a_self_contained_error_page() {
    let upstream = Upstream::new();
    let wiki = upstream.wiki().await;
    let reply = get(&wiki, "/").await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    let html = reply.text();
    assert!(html.contains("503 Service Unavailable"), "{html}");
    assert!(!html.contains("<script"), "{html}");
}

#[tokio::test]
async fn page_html_is_safe_mode() {
    let fx = wiki_with(&[("README.md", "<script>alert(1)</script>\n\n[x](javascript:alert(1))\n")]).await;
    let html = get(&fx.wiki, "/").await.text();
    assert!(!html.contains("<script>alert"), "{html}");
    assert!(!html.contains("javascript:"), "{html}");
}
