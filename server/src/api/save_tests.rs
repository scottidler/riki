//! The write path end to end through the router: local `file://` upstreams in tempdirs, never the
//! network. Push stalls and failures come from a `remote.origin.receivepack` wrapper script set on
//! the test's own bare clone, so no user or global git config is touched.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use riki_core::runtime::Runtime;
use riki_core::save::SaveSettings;
use riki_core::store::Signer;
use riki_core::testing::{file_at, head, head_author_email, history_len, set_receive_pack, write_script};
use riki_core::wiki::Wiki;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::routes::{AppState, router};
use crate::testkit::{BRANCH, Upstream};

const TIMEOUT: Duration = Duration::from_secs(30);

fn app(wiki: &Arc<Wiki>, push_retries: u32) -> Router {
    let save = SaveSettings {
        committer: Signer {
            name: "riki".to_string(),
            email: "riki@localhost".to_string(),
        },
        push_retries,
    };
    router(AppState::new(Arc::new(Runtime::new()), wiki.clone()).with_save(save))
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let body = response.into_body().collect().await.expect("body").to_bytes().to_vec();
    (status, body)
}

fn json_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or_else(|err| panic!("not JSON ({err}): {}", String::from_utf8_lossy(bytes)))
}

async fn save(app: &Router, path: &str, base_oid: Option<&str>, body: &str) -> (StatusCode, Value) {
    post(
        app,
        "/_riki/api/page",
        json!({"path": path, "base-oid": base_oid, "body": body}),
    )
    .await
}

async fn post(app: &Router, route: &str, payload: Value) -> (StatusCode, Value) {
    let request = Request::post(route)
        .header("content-type", "application/json")
        .header("remote-email", "alice@example.com")
        .header("remote-name", "Alice")
        .body(Body::from(payload.to_string()))
        .expect("request");
    let (status, body) = send(app, request).await;
    (status, json_of(&body))
}

async fn load(app: &Router, path: &str) -> (StatusCode, Value) {
    let request = Request::get(format!("/_riki/api/page?path={path}"))
        .body(Body::empty())
        .expect("request");
    let (status, body) = send(app, request).await;
    (status, json_of(&body))
}

async fn base_oid(app: &Router, path: &str) -> String {
    let (status, page) = load(app, path).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    page["base-oid"].as_str().expect("an existing page").to_string()
}

async fn html(app: &Router, url: &str) -> (StatusCode, String) {
    let (status, body) = send(app, Request::get(url).body(Body::empty()).expect("request")).await;
    (status, String::from_utf8_lossy(&body).into_owned())
}

fn file(upstream: &Upstream, path: &str) -> Option<String> {
    file_at(&upstream.dir, BRANCH, path).map(|bytes| String::from_utf8(bytes).expect("utf-8"))
}

async fn wait_for(path: &Path) {
    for _ in 0..300 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{} never appeared", path.display());
}

#[tokio::test]
async fn front_matter_and_trailing_newline_state_are_preserved_byte_exact() {
    let upstream = Upstream::new();
    upstream.push(&[
        ("fm.md", "---\ntitle: T\ntags: [a]\n---\n\n# Old\n\nbody"),
        ("two.md", "# Two\n\n"),
        ("one.md", "# One\n"),
    ]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki, 1);

    let (_, page) = load(&app, "fm.md").await;
    assert_eq!(page["body"], "# Old\n\nbody", "front matter is never sent");
    assert_eq!(page["editable"], true);
    let base = base_oid(&app, "fm.md").await;
    let (status, reply) = save(&app, "fm.md", Some(&base), "# New\n\nbody\n").await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(
        file(&upstream, "fm.md").as_deref(),
        Some("---\ntitle: T\ntags: [a]\n---\n\n# New\n\nbody"),
        "front matter re-attached, no trailing newline kept"
    );

    let base = base_oid(&app, "two.md").await;
    save(&app, "two.md", Some(&base), "# Two edited\n").await;
    assert_eq!(file(&upstream, "two.md").as_deref(), Some("# Two edited\n\n"));

    let base = base_oid(&app, "one.md").await;
    save(&app, "one.md", Some(&base), "# One edited").await;
    assert_eq!(file(&upstream, "one.md").as_deref(), Some("# One edited\n"));

    let (status, _) = save(&app, "new.md", None, "fresh\n\n\n").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        file(&upstream, "new.md").as_deref(),
        Some("fresh\n"),
        "new pages end with one newline"
    );
    assert_eq!(head_author_email(&upstream.dir, BRANCH), "alice@example.com");
}

#[tokio::test]
async fn get_right_after_a_200_renders_the_saved_content() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n")]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki, 1);
    let base = base_oid(&app, "README.md").await;
    let (status, reply) = save(&app, "README.md", Some(&base), "# Fresh words\n").await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert!(reply["commit"].is_string(), "{reply}");
    assert_eq!(reply["content-present"], false);
    let (status, page) = html(&app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Fresh words"), "{page}");

    let (status, _) = save(&app, "docs/created.md", None, "Brand new page").await;
    assert_eq!(status, StatusCode::OK);
    let (status, page) = html(&app, "/docs/created").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Brand new page"), "{page}");
}

#[tokio::test]
async fn push_timeout_is_503_retry_safe_then_retry_is_200_content_present_with_one_commit() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n")]);
    let wiki = upstream.replica("r", Duration::from_secs(2)).await;
    // The push lands upstream, then the remote side hangs past `git.timeout`.
    let script = upstream.tmp.path().join("hang-after-receive.sh");
    write_script(&script, "git receive-pack \"$@\"\nsleep 10");
    set_receive_pack(&upstream.clone_dir("r"), &script);
    let app = app(&wiki, 1);
    let base = base_oid(&app, "README.md").await;

    let (status, reply) = save(&app, "README.md", Some(&base), "# saved once\n").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{reply}");
    assert_eq!(reply["retry-safe"], true, "{reply}");
    assert_eq!(history_len(&upstream.dir, BRANCH), 2, "the timed-out push landed");

    let (status, reply) = save(&app, "README.md", Some(&base), "# saved once\n").await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["content-present"], true, "{reply}");
    assert_eq!(reply["commit"], Value::Null);
    assert_eq!(history_len(&upstream.dir, BRANCH), 2, "exactly one commit upstream");
    let (_, page) = html(&app, "/").await;
    assert!(page.contains("saved once"), "content-present publishes the tip: {page}");
}

#[tokio::test]
async fn a_crlf_page_is_not_editable_and_its_save_is_422() {
    let upstream = Upstream::new();
    upstream.push_bytes(&[("README.md", b"# home\n"), ("crlf.md", b"# Windows\r\n\r\nline\r\n")]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki, 1);
    let (status, page) = load(&app, "crlf.md").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["editable"], false);
    assert_eq!(page["body"], Value::Null);
    assert!(
        page["reason"].as_str().expect("reason").contains("carriage return"),
        "{page}"
    );
    let base = page["base-oid"].as_str().expect("oid").to_string();

    let (status, reply) = save(&app, "crlf.md", Some(&base), "# Windows\n\nline\n").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    assert!(
        reply["error"].as_str().expect("error").contains("carriage return"),
        "{reply}"
    );
    assert_eq!(history_len(&upstream.dir, BRANCH), 1, "nothing committed");
}

#[tokio::test]
async fn two_saves_of_one_page_from_one_base_one_200_one_409_one_new_commit() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n")]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki, 1);
    let base = base_oid(&app, "README.md").await;
    let (first, second) = tokio::join!(
        save(&app, "README.md", Some(&base), "# from first\n"),
        save(&app, "README.md", Some(&base), "# from second\n"),
    );
    let mut codes = [first.0, second.0];
    codes.sort();
    assert_eq!(codes, [StatusCode::OK, StatusCode::CONFLICT], "{first:?} {second:?}");
    let (winner, loser) = if first.0 == StatusCode::OK {
        ("# from first\n", second.1)
    } else {
        ("# from second\n", first.1)
    };
    assert_eq!(loser["current-body"], winner, "409 carries the current body");
    assert_eq!(history_len(&upstream.dir, BRANCH), 2, "one new upstream commit");
    assert_eq!(file(&upstream, "README.md").as_deref(), Some(winner));
}

#[tokio::test]
async fn two_replicas_saving_one_page_from_one_base_one_200_one_409() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n")]);
    let (a, b) = (
        upstream.replica("a", TIMEOUT).await,
        upstream.replica("b", TIMEOUT).await,
    );
    let (app_a, app_b) = (app(&a, 1), app(&b, 1));
    let base = base_oid(&app_a, "README.md").await;
    let (first, second) = tokio::join!(
        save(&app_a, "README.md", Some(&base), "# from a\n"),
        save(&app_b, "README.md", Some(&base), "# from b\n"),
    );
    let mut codes = [first.0, second.0];
    codes.sort();
    assert_eq!(codes, [StatusCode::OK, StatusCode::CONFLICT], "{first:?} {second:?}");
    assert_eq!(history_len(&upstream.dir, BRANCH), 2, "one new upstream commit");
}

/// Replica `b`'s push waits until replica `a` has saved, so `b` pushes from a stale tip.
struct Race {
    upstream: Upstream,
    app_a: Router,
    app_b: Router,
    waiting: std::path::PathBuf,
    released: std::path::PathBuf,
    log: std::path::PathBuf,
}

async fn race(b_push_retries: u32) -> Race {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n")]);
    let a = upstream.replica("a", TIMEOUT).await;
    let b = upstream.replica("b", TIMEOUT).await;
    let root = upstream.tmp.path();
    let (waiting, released, log) = (root.join("b-waiting"), root.join("b-released"), root.join("b-pushes"));
    let script = root.join("held-receive-pack.sh");
    write_script(
        &script,
        &format!(
            "echo push >> '{log}'\nif [ ! -e '{released}' ]; then\n  touch '{waiting}'\n  i=0\n  \
             while [ ! -e '{released}' ] && [ $i -lt 300 ]; do sleep 0.1; i=$((i+1)); done\nfi\n\
             exec git receive-pack \"$@\"",
            log = log.display(),
            released = released.display(),
            waiting = waiting.display(),
        ),
    );
    set_receive_pack(&upstream.clone_dir("b"), &script);
    Race {
        app_a: app(&a, 1),
        app_b: app(&b, b_push_retries),
        upstream,
        waiting,
        released,
        log,
    }
}

impl Race {
    /// `b` saves `b.md` while `a` saves `a.md` between `b`'s fetch and `b`'s push.
    async fn run(&self) -> (StatusCode, Value) {
        let app_b = self.app_b.clone();
        let b = tokio::spawn(async move { save(&app_b, "b.md", None, "from b\n").await });
        wait_for(&self.waiting).await;
        let (status, reply) = save(&self.app_a, "a.md", None, "from a\n").await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        std::fs::write(&self.released, "").expect("release b");
        b.await.expect("b's save")
    }

    fn pushes(&self) -> usize {
        std::fs::read_to_string(&self.log).expect("push log").lines().count()
    }
}

#[tokio::test]
async fn different_file_saves_on_two_replicas_both_200_after_one_retry() {
    let race = race(1).await;
    let (status, reply) = race.run().await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(race.pushes(), 2, "b pushed, was rejected, fetched, and pushed again");
    let upstream = &race.upstream;
    assert_eq!(history_len(&upstream.dir, BRANCH), 3, "seed + a + b, linear");
    assert_eq!(file(upstream, "a.md").as_deref(), Some("from a\n"));
    assert_eq!(file(upstream, "b.md").as_deref(), Some("from b\n"));
}

#[tokio::test]
async fn non_fast_forward_past_push_retries_is_409() {
    let race = race(0).await;
    let (status, reply) = race.run().await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert!(
        reply["error"].as_str().expect("error").contains("push rejected"),
        "{reply}"
    );
    assert_eq!(race.pushes(), 1);
    assert_eq!(history_len(&race.upstream.dir, BRANCH), 2, "only a's commit");
    assert_eq!(file(&race.upstream, "b.md"), None);
}

#[tokio::test]
async fn creating_a_b_md_while_upstream_gained_a_b_readme_is_409_and_nothing_pushed() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n")]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki, 1);
    let laptop = upstream.push(&[("a/b/README.md", "# from the laptop\n")]);
    let (status, reply) = save(&app, "a/b.md", None, "# from the browser\n").await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    let error = reply["error"].as_str().expect("error");
    assert!(
        error.contains("a/b.md") && error.contains("a/b/README.md"),
        "names the error: {error}"
    );
    assert_eq!(head(&upstream.dir, BRANCH), Some(laptop), "nothing pushed");
    assert_eq!(file(&upstream, "a/b.md"), None);
}

#[tokio::test]
async fn any_other_push_failure_is_502_with_gits_stderr() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n")]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let script = upstream.tmp.path().join("refuse.sh");
    write_script(&script, "echo 'denied by the test remote' >&2\nexit 1");
    set_receive_pack(&upstream.clone_dir("r"), &script);
    let app = app(&wiki, 1);
    let (status, reply) = save(&app, "x.md", None, "x").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{reply}");
    assert!(
        reply["error"]
            .as_str()
            .expect("error")
            .contains("denied by the test remote"),
        "{reply}"
    );
    assert_eq!(history_len(&upstream.dir, BRANCH), 1);
}

#[tokio::test]
async fn upstream_unreachable_is_503_and_nothing_committed() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n")]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki, 1);
    let away = upstream.tmp.path().join("away.git");
    std::fs::rename(&upstream.dir, &away).expect("take upstream away");
    let (status, reply) = save(&app, "x.md", None, "x").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{reply}");
    assert_eq!(reply.get("retry-safe"), None, "a fetch failure commits nothing");
    std::fs::rename(&away, &upstream.dir).expect("bring upstream back");
    assert_eq!(history_len(&upstream.dir, BRANCH), 1);
}

#[tokio::test]
async fn saving_the_same_body_is_200_with_no_commit() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n")]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki, 1);
    let base = base_oid(&app, "README.md").await;
    let (status, reply) = save(&app, "README.md", Some(&base), "# home").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply, json!({"commit": null, "content-present": false}));
    assert_eq!(history_len(&upstream.dir, BRANCH), 1);
}

#[tokio::test]
async fn bad_requests_are_400() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n")]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki, 1);
    for (path, base) in [
        ("x.txt", None),
        ("../x.md", None),
        ("health.md", None),
        ("x.md", Some("zz")),
    ] {
        let (status, reply) = save(&app, path, base, "x").await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {reply}");
    }
    let (status, _) = post(&app, "/_riki/api/page", json!({"path": "x.md"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "missing body field");
    let (status, _) = load(&app, "../x.md").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(
        &app,
        Request::get("/_riki/api/page").body(Body::empty()).expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "missing path");
    assert_eq!(history_len(&upstream.dir, BRANCH), 1);
}

#[tokio::test]
async fn load_of_a_missing_page_has_a_null_base_oid() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n")]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let (status, page) = load(&app(&wiki, 1), "docs/missing.md").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        page,
        json!({"path": "docs/missing.md", "base-oid": null, "body": "", "editable": true, "reason": null})
    );
}

#[tokio::test]
async fn roundtrip_compares_with_the_body_of_the_base_blob() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "---\ntitle: x\n---\n# home\n\nline two\n")]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki, 1);
    let base = base_oid(&app, "README.md").await;
    let check = |serialized: &str| json!({"path": "README.md", "base-oid": base, "serialized": serialized});
    let (status, reply) = post(&app, "/_riki/api/roundtrip", check("# home\n\nline two")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply, json!({"identical": true, "first-diff-line": null}));
    let (status, reply) = post(&app, "/_riki/api/roundtrip", check("# home\n\nline 2\n")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply, json!({"identical": false, "first-diff-line": 3}));
    let missing =
        json!({"path": "README.md", "base-oid": "0000000000000000000000000000000000000001", "serialized": ""});
    let (status, _) = post(&app, "/_riki/api/roundtrip", missing).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let bad = json!({"path": "README.md", "base-oid": "nope", "serialized": ""});
    let (status, _) = post(&app, "/_riki/api/roundtrip", bad).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
