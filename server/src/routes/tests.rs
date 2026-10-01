use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

use super::*;

async fn get_json(runtime: Arc<Runtime>, path: &str) -> (StatusCode, Value) {
    let app = router(AppState::new(runtime));
    let response = app
        .oneshot(Request::get(path).body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = response.into_body().collect().await.expect("body").to_bytes();
    (status, serde_json::from_slice(&bytes).expect("json"))
}

fn keys(value: &Value) -> String {
    let mut keys: Vec<&str> = value.as_object().expect("object").keys().map(String::as_str).collect();
    keys.sort_unstable();
    keys.join(",")
}

#[tokio::test]
async fn version_has_the_standard_keys() {
    let (code, body) = get_json(Arc::new(Runtime::new()), "/version").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(keys(&body), "branch,git_sha,revision,version");
}

#[tokio::test]
async fn health_is_ok() {
    let (code, body) = get_json(Arc::new(Runtime::new()), "/health").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn ready_is_503_until_latched() {
    let runtime = Arc::new(Runtime::new());
    let (code, body) = get_json(runtime.clone(), "/ready").await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["ready"], false);
    runtime.mark_ready();
    let (code, body) = get_json(runtime, "/ready").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["ready"], true);
}

#[tokio::test]
async fn status_and_deployed_shapes() {
    let runtime = Arc::new(Runtime::new());
    let (_, status) = get_json(runtime.clone(), "/status").await;
    assert_eq!(keys(&status), "status,uptime");
    assert_eq!(status["status"], "ok");
    let (_, deployed) = get_json(runtime, "/deployed").await;
    assert_eq!(keys(&deployed), "deployed_at,deployer,environment");
}

#[tokio::test]
async fn unknown_path_is_404_until_the_page_route_lands() {
    let app = router(AppState::new(Arc::new(Runtime::new())));
    let response = app
        .oneshot(Request::get("/nope").body(Body::empty()).expect("request"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
