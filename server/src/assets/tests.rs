use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::*;

fn app() -> Router {
    Router::new().route("/_riki/assets/{name}", get(asset))
}

async fn send(path: &str, if_none_match: Option<&str>) -> axum::response::Response {
    let mut request = Request::builder().uri(path);
    if let Some(tag) = if_none_match {
        request = request.header(header::IF_NONE_MATCH, tag);
    }
    app()
        .oneshot(request.body(Body::empty()).expect("request"))
        .await
        .expect("response")
}

#[test]
fn the_committed_bundle_is_embedded() {
    let js = find("editor.js").expect("editor.js");
    assert!(js.bytes.len() > 1000, "editor.js is {} bytes", js.bytes.len());
    assert!(find("editor.css").is_some());
}

#[tokio::test]
async fn serves_the_editor_bundle_with_its_type() {
    let reply = send("/_riki/assets/editor.js", None).await;
    assert_eq!(reply.status(), StatusCode::OK);
    assert_eq!(reply.headers()[header::CONTENT_TYPE], "text/javascript; charset=utf-8");
    assert_eq!(reply.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    let body = reply.into_body().collect().await.expect("body").to_bytes();
    assert_eq!(&body[..], find("editor.js").expect("editor.js").bytes);
}

#[tokio::test]
async fn serves_the_stylesheet_as_css() {
    let reply = send("/_riki/assets/editor.css", None).await;
    assert_eq!(reply.status(), StatusCode::OK);
    assert_eq!(reply.headers()[header::CONTENT_TYPE], "text/css; charset=utf-8");
}

#[tokio::test]
async fn unknown_assets_are_404() {
    for path in [
        "/_riki/assets/nope.js",
        "/_riki/assets/editor.js.map",
        "/_riki/assets/..%2Fmain.rs",
    ] {
        assert_eq!(send(path, None).await.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn a_matching_etag_is_304_and_a_stale_one_is_200() {
    let first = send("/_riki/assets/editor.js", None).await;
    let tag = first.headers()[header::ETAG].to_str().expect("etag").to_string();
    assert_eq!(
        send("/_riki/assets/editor.js", Some(&tag)).await.status(),
        StatusCode::NOT_MODIFIED
    );
    assert_eq!(
        send("/_riki/assets/editor.js", Some("\"stale\"")).await.status(),
        StatusCode::OK
    );
}
