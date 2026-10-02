use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

use super::*;
use crate::testkit::Upstream;

async fn get_json(state: AppState, path: &str) -> (StatusCode, Value) {
    let app = router(state);
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

/// A wiki that has published one healthy commit; `upstream` keeps the tempdir alive.
struct Healthy {
    upstream: Upstream,
    wiki: Arc<Wiki>,
}

async fn healthy() -> Healthy {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n")]);
    let wiki = upstream.wiki().await;
    wiki.poll().await.expect("poll");
    Healthy { upstream, wiki }
}

#[tokio::test]
async fn version_has_the_standard_keys() {
    let fx = healthy().await;
    let wiki = fx.wiki.clone();
    let (code, body) = get_json(AppState::new(Arc::new(Runtime::new()), wiki), "/version").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(keys(&body), "branch,git_sha,revision,version");
}

#[tokio::test]
async fn health_is_ok() {
    let fx = healthy().await;
    let wiki = fx.wiki.clone();
    let (code, body) = get_json(AppState::new(Arc::new(Runtime::new()), wiki), "/health").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn ready_is_503_until_latched() {
    let fx = healthy().await;
    let wiki = fx.wiki.clone();
    let runtime = Arc::new(Runtime::new());
    let state = AppState::new(runtime.clone(), wiki);
    let (code, body) = get_json(state.clone(), "/ready").await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["ready"], false);
    runtime.mark_ready();
    let (code, body) = get_json(state, "/ready").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body["ready"], true);
}

#[tokio::test]
async fn status_and_deployed_shapes() {
    let fx = healthy().await;
    let wiki = fx.wiki.clone();
    let state = AppState::new(Arc::new(Runtime::new()), wiki);
    let (_, status) = get_json(state.clone(), "/status").await;
    assert_eq!(keys(&status), "status,uptime");
    assert_eq!(status["status"], "ok");
    let (_, deployed) = get_json(state, "/deployed").await;
    assert_eq!(keys(&deployed), "deployed_at,deployer,environment");
}

#[tokio::test]
async fn status_is_degraded_when_the_tip_fails_publish() {
    let fx = healthy().await;
    let (upstream, wiki) = (&fx.upstream, fx.wiki.clone());
    let good = wiki.good().expect("good").commit();
    let bad = upstream.push(&[("a.md", "file\n"), ("a/README.md", "dir\n")]);
    wiki.poll().await.expect("poll");
    assert_eq!(wiki.good().expect("good").commit(), good, "good tip unchanged");
    let (code, status) = get_json(AppState::new(Arc::new(Runtime::new()), wiki), "/status").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(status["status"], "degraded");
    let error = status["error"].as_str().expect("error string");
    assert!(error.contains(&bad.to_string()), "{error}");
}

#[tokio::test]
async fn status_is_degraded_while_upstream_is_unreachable_then_recovers() {
    let fx = healthy().await;
    let (upstream, wiki) = (&fx.upstream, fx.wiki.clone());
    let state = AppState::new(Arc::new(Runtime::new()), wiki.clone());
    let parked = upstream.tmp.path().join("parked.git");
    std::fs::rename(&upstream.dir, &parked).expect("park upstream");
    wiki.poll().await.expect("poll");
    let (_, status) = get_json(state.clone(), "/status").await;
    assert_eq!(status["status"], "degraded");
    assert!(
        status["error"]
            .as_str()
            .expect("error")
            .starts_with("upstream unreachable since "),
        "{status}"
    );
    std::fs::rename(&parked, &upstream.dir).expect("restore upstream");
    wiki.poll().await.expect("poll");
    let (_, status) = get_json(state, "/status").await;
    assert_eq!(keys(&status), "status,uptime");
    assert_eq!(status["status"], "ok");
}

#[test]
fn save_settings_come_from_the_committer_and_git_config() {
    let config = crate::config::Config::from_yaml(
        "content:\n  remote: x\ngit:\n  push-retries: 3\ncommitter:\n  name: bot\n  email: bot@example.com\n",
        Some(std::path::Path::new("/home/test")),
    )
    .expect("loads");
    let settings = save_settings(&config.committer, &config.git);
    assert_eq!(settings.committer.name, "bot");
    assert_eq!(settings.committer.email, "bot@example.com");
    assert_eq!(settings.push_retries, 3);
}
