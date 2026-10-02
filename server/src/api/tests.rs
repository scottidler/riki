use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use super::*;
use crate::routes::router as app_router;
use crate::testkit::Upstream;

async fn post(path: &str, content_type: Option<&str>, email: Option<&str>) -> StatusCode {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n")]);
    let wiki = upstream.wiki().await;
    let state = AppState::new(std::sync::Arc::new(riki_core::runtime::Runtime::new()), wiki);
    let mut request = Request::post(path);
    if let Some(content_type) = content_type {
        request = request.header("content-type", content_type);
    }
    if let Some(email) = email {
        request = request.header("remote-email", email);
    }
    app_router(state)
        .oneshot(request.body(Body::from("{}")).expect("request"))
        .await
        .expect("response")
        .status()
}

const POST_ROUTES: [&str; 4] = [
    "/_riki/api/page",
    "/_riki/api/roundtrip",
    "/_riki/api/delete",
    "/_riki/api/restore",
];

#[tokio::test]
async fn save_without_the_email_header_is_401() {
    let code = post("/_riki/api/page", Some("application/json"), None).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn save_with_the_email_header_passes_the_guards() {
    let code = post("/_riki/api/page", Some("application/json"), Some("a@x.com")).await;
    assert_eq!(
        code,
        StatusCode::BAD_REQUEST,
        "reaches the handler, which refuses an empty object"
    );
}

#[tokio::test]
async fn form_encoded_post_is_415_on_every_post_route() {
    for path in POST_ROUTES {
        let code = post(path, Some("application/x-www-form-urlencoded"), Some("a@x.com")).await;
        assert_eq!(code, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{path}");
    }
}

#[tokio::test]
async fn delete_and_restore_without_the_email_header_are_401() {
    for path in ["/_riki/api/delete", "/_riki/api/restore"] {
        let code = post(path, Some("application/json"), None).await;
        assert_eq!(code, StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn missing_content_type_is_415() {
    let code = post("/_riki/api/page", None, Some("a@x.com")).await;
    assert_eq!(code, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn json_with_charset_parameter_is_accepted() {
    for path in POST_ROUTES {
        let code = post(path, Some("application/json; charset=utf-8"), Some("a@x.com")).await;
        assert_eq!(
            code,
            StatusCode::BAD_REQUEST,
            "{path}: past the guard, `{{}}` is refused"
        );
    }
}

#[tokio::test]
async fn the_json_check_runs_before_the_identity_check() {
    let code = post("/_riki/api/page", Some("text/plain"), None).await;
    assert_eq!(code, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[test]
fn is_json_matches_the_essence_only() {
    assert!(is_json(Some("application/json")));
    assert!(is_json(Some("Application/JSON ; charset=utf-8")));
    assert!(!is_json(Some("application/jsonx")));
    assert!(!is_json(Some("text/json")));
    assert!(!is_json(None));
}
