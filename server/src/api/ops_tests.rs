//! Move, delete, and restore end to end through the router: local `file://` upstreams in
//! tempdirs, push stalls from a `remote.origin.receivepack` wrapper on the test's own bare clone.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use riki_core::runtime::Runtime;
use riki_core::save::SaveSettings;
use riki_core::store::Signer;
use riki_core::testing::{commit, file_at, head, head_author_email, history_len, set_receive_pack, write_script};
use riki_core::wiki::Wiki;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::routes::{AppState, router};
use crate::testkit::{BRANCH, Upstream};

const TIMEOUT: Duration = Duration::from_secs(30);
const PAGE: &str = "# Guide\n\nThe original words.\n";

fn app(wiki: &Arc<Wiki>) -> Router {
    let save = SaveSettings {
        committer: Signer {
            name: "riki".to_string(),
            email: "riki@localhost".to_string(),
        },
        push_retries: 1,
    };
    router(AppState::new(Arc::new(Runtime::new()), wiki.clone()).with_save(save))
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let body = response.into_body().collect().await.expect("body").to_bytes().to_vec();
    (status, body)
}

async fn post(app: &Router, route: &str, payload: Value) -> (StatusCode, Value) {
    let request = Request::post(route)
        .header("content-type", "application/json")
        .header("remote-email", "alice@example.com")
        .header("remote-name", "Alice")
        .body(Body::from(payload.to_string()))
        .expect("request");
    let (status, body) = send(app, request).await;
    let value = serde_json::from_slice(&body)
        .unwrap_or_else(|err| panic!("not JSON ({err}): {}", String::from_utf8_lossy(&body)));
    (status, value)
}

async fn base_oid(app: &Router, path: &str) -> String {
    let request = Request::get(format!("/_riki/api/page?path={path}"))
        .body(Body::empty())
        .expect("request");
    let (status, body) = send(app, request).await;
    assert_eq!(status, StatusCode::OK);
    let page: Value = serde_json::from_slice(&body).expect("json");
    page["base-oid"].as_str().expect("an existing page").to_string()
}

async fn delete(app: &Router, path: &str, base: &str) -> (StatusCode, Value) {
    post(app, "/_riki/api/delete", json!({"path": path, "base-oid": base})).await
}

async fn restore(app: &Router, path: &str, commit: &str) -> (StatusCode, Value) {
    post(app, "/_riki/api/restore", json!({"path": path, "commit": commit})).await
}

async fn html(app: &Router, url: &str) -> (StatusCode, String) {
    let (status, body) = send(app, Request::get(url).body(Body::empty()).expect("request")).await;
    (status, String::from_utf8_lossy(&body).into_owned())
}

fn seeded() -> Upstream {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n"), ("guide.md", PAGE)]);
    upstream
}

/// Make replica `name`'s pushes land upstream, then hang past a 2s `git.timeout`.
fn hang_after_receive(upstream: &Upstream, name: &str) {
    let script = upstream.tmp.path().join("hang-after-receive.sh");
    write_script(&script, "git receive-pack \"$@\"\nsleep 10");
    set_receive_pack(&upstream.clone_dir(name), &script);
}

#[tokio::test]
async fn delete_then_restore_is_two_upstream_commits_and_the_original_bytes() {
    let upstream = seeded();
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki);
    let base = base_oid(&app, "guide.md").await;

    let (status, reply) = delete(&app, "guide.md", &base).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["content-present"], false);
    let d = reply["commit"].as_str().expect("a commit").to_string();
    assert_eq!(reply.get("url"), None, "delete answers {{commit}} only");
    assert_eq!(file_at(&upstream.dir, BRANCH, "guide.md"), None);
    assert_eq!(head_author_email(&upstream.dir, BRANCH), "alice@example.com");
    let (status, _) = html(&app, "/guide").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the delete is published");

    let (status, reply) = restore(&app, "guide.md", &d).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert!(reply["commit"].is_string(), "{reply}");
    assert_eq!(reply["url"], "/guide");
    assert_eq!(history_len(&upstream.dir, BRANCH), 3, "exactly two new commits");
    assert_eq!(
        file_at(&upstream.dir, BRANCH, "guide.md"),
        Some(PAGE.as_bytes().to_vec())
    );
    let (status, page) = html(&app, "/guide").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("The original words."), "{page}");
}

#[tokio::test]
async fn delete_with_a_stale_base_oid_is_409_and_nothing_pushed() {
    let upstream = seeded();
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki);
    let base = base_oid(&app, "guide.md").await;
    let laptop = upstream.push(&[("guide.md", "# Guide\n\nEdited on a laptop.\n")]);
    let (status, reply) = delete(&app, "guide.md", &base).await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(head(&upstream.dir, BRANCH), Some(laptop), "nothing pushed");
}

#[tokio::test]
async fn restore_after_recreation_is_409_for_other_bytes_and_200_content_present_for_the_same() {
    let upstream = seeded();
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki);
    let base = base_oid(&app, "guide.md").await;
    let (_, reply) = delete(&app, "guide.md", &base).await;
    let d = reply["commit"].as_str().expect("a commit").to_string();

    let other = upstream.push(&[("guide.md", "# Someone else's guide\n")]);
    let (status, reply) = restore(&app, "guide.md", &d).await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(head(&upstream.dir, BRANCH), Some(other), "nothing pushed");

    let same = upstream.push(&[("guide.md", PAGE)]);
    let (status, reply) = restore(&app, "guide.md", &d).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply, json!({"commit": null, "content-present": true, "url": "/guide"}));
    assert_eq!(head(&upstream.dir, BRANCH), Some(same), "no commit");
}

#[tokio::test]
async fn delete_push_timeout_is_503_retry_safe_then_retry_is_200_content_present() {
    let upstream = seeded();
    let wiki = upstream.replica("r", Duration::from_secs(2)).await;
    hang_after_receive(&upstream, "r");
    let app = app(&wiki);
    let base = base_oid(&app, "guide.md").await;

    let (status, reply) = delete(&app, "guide.md", &base).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{reply}");
    assert_eq!(reply["retry-safe"], true, "{reply}");
    assert_eq!(history_len(&upstream.dir, BRANCH), 2, "the timed-out push landed");

    let (status, reply) = delete(&app, "guide.md", &base).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply, json!({"commit": null, "content-present": true}));
    assert_eq!(history_len(&upstream.dir, BRANCH), 2, "exactly one delete commit");
    let (status, _) = html(&app, "/guide").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "content-present published the tip");
}

#[tokio::test]
async fn restore_push_timeout_is_503_retry_safe_then_retry_is_200_content_present() {
    let upstream = seeded();
    let wiki = upstream.replica("r", Duration::from_secs(2)).await;
    let app = app(&wiki);
    let base = base_oid(&app, "guide.md").await;
    let (_, reply) = delete(&app, "guide.md", &base).await;
    let d = reply["commit"].as_str().expect("a commit").to_string();
    hang_after_receive(&upstream, "r");

    let (status, reply) = restore(&app, "guide.md", &d).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{reply}");
    assert_eq!(reply["retry-safe"], true, "{reply}");
    assert_eq!(history_len(&upstream.dir, BRANCH), 3, "the timed-out push landed");

    let (status, reply) = restore(&app, "guide.md", &d).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply, json!({"commit": null, "content-present": true, "url": "/guide"}));
    assert_eq!(history_len(&upstream.dir, BRANCH), 3, "exactly one restore commit");
    let (status, page) = html(&app, "/guide").await;
    assert_eq!(status, StatusCode::OK, "content-present published the tip");
    assert!(page.contains("The original words."), "{page}");
}

#[tokio::test]
async fn bad_requests_are_400_and_commit_nothing() {
    let upstream = seeded();
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki);
    let readme = base_oid(&app, "README.md").await;
    let guide = base_oid(&app, "guide.md").await;
    let (status, reply) = delete(&app, "README.md", &readme).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{reply}");
    assert!(reply["error"].as_str().expect("error").contains("home page"), "{reply}");
    for (path, base) in [
        ("guide.txt", guide.as_str()),
        ("../guide.md", &guide),
        ("guide.md", "zz"),
    ] {
        let (status, reply) = delete(&app, path, base).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {reply}");
    }
    let (status, _) = post(&app, "/_riki/api/delete", json!({"path": "guide.md"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "missing base-oid");
    let (status, reply) = restore(&app, "guide.md", "nope").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{reply}");
    assert!(
        reply["error"].as_str().expect("error").starts_with("commit "),
        "{reply}"
    );
    let (status, reply) = restore(&app, "guide.md", &guide).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a blob is not a delete commit: {reply}"
    );
    let seed = head(&upstream.dir, BRANCH).expect("seed").to_string();
    let (status, reply) = restore(&app, "guide.md", &seed).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "the seed deleted nothing: {reply}");
    assert_eq!(history_len(&upstream.dir, BRANCH), 1);
}

#[tokio::test]
async fn delete_with_upstream_unreachable_is_a_plain_503() {
    let upstream = seeded();
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki);
    let base = base_oid(&app, "guide.md").await;
    let away = upstream.tmp.path().join("away.git");
    std::fs::rename(&upstream.dir, &away).expect("take upstream away");
    let (status, reply) = delete(&app, "guide.md", &base).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{reply}");
    assert_eq!(reply.get("retry-safe"), None, "a fetch failure commits nothing");
    std::fs::rename(&away, &upstream.dir).expect("bring upstream back");
    assert_eq!(history_len(&upstream.dir, BRANCH), 1);
}

#[tokio::test]
async fn a_restore_colliding_with_a_folder_readme_is_409_index_conflict() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n"), ("a/b.md", "# B\n")]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki);
    let base = base_oid(&app, "a/b.md").await;
    let (_, reply) = delete(&app, "a/b.md", &base).await;
    let d = reply["commit"].as_str().expect("a commit").to_string();
    let laptop = commit(
        &upstream.dir,
        BRANCH,
        &[("a/b/README.md", Some(b"# B folder\n"))],
        "laptop",
    );
    let (status, reply) = restore(&app, "a/b.md", &d).await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    let error = reply["error"].as_str().expect("error");
    assert!(error.starts_with("the restore would break the wiki"), "{error}");
    assert_eq!(head(&upstream.dir, BRANCH), Some(laptop), "nothing pushed");
}

async fn move_page(app: &Router, from: &str, base: &str, to: &str) -> (StatusCode, Value) {
    post(
        app,
        "/_riki/api/move",
        json!({"from": from, "base-oid": base, "to": to}),
    )
    .await
}

/// The status and `Location` of a GET of `url`.
async fn location(app: &Router, url: &str) -> (StatusCode, Option<String>) {
    let request = Request::get(url).body(Body::empty()).expect("request");
    let response = app.clone().oneshot(request).await.expect("response");
    let location = response
        .headers()
        .get("location")
        .map(|value| value.to_str().expect("ascii").to_string());
    (response.status(), location)
}

/// `git diff -M --name-status <branch>~1 <branch>` in the bare repo at `dir`.
fn name_status(dir: &std::path::Path) -> String {
    let output = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(dir)
        .args(["diff", "-M", "--name-status"])
        .arg(format!("{BRANCH}~1"))
        .arg(BRANCH)
        .output()
        .expect("git diff");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).expect("utf-8")
}

#[tokio::test]
async fn a_move_is_one_r100_commit_and_the_old_url_301s_to_the_new() {
    let upstream = seeded();
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki);
    let base = base_oid(&app, "guide.md").await;

    let (status, reply) = move_page(&app, "guide.md", &base, "docs/handbook.md").await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["content-present"], false);
    assert!(reply["commit"].is_string(), "{reply}");
    assert_eq!(reply["url"], "/docs/handbook");
    assert_eq!(history_len(&upstream.dir, BRANCH), 2, "exactly one commit");
    assert_eq!(name_status(&upstream.dir), "R100\tguide.md\tdocs/handbook.md\n");
    assert_eq!(head_author_email(&upstream.dir, BRANCH), "alice@example.com");

    assert_eq!(
        location(&app, "/guide").await,
        (StatusCode::MOVED_PERMANENTLY, Some("/docs/handbook".to_string()))
    );
    let (status, page) = html(&app, "/docs/handbook").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("The original words."), "{page}");
}

#[tokio::test]
async fn a_move_onto_a_page_or_into_a_readme_collision_is_409_and_nothing_pushed() {
    let upstream = Upstream::new();
    upstream.push(&[
        ("README.md", "# home\n"),
        ("guide.md", PAGE),
        ("other.md", "# Other\n"),
        ("team/README.md", "# Team\n"),
    ]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki);
    let base = base_oid(&app, "guide.md").await;
    for to in ["other.md", "team.md"] {
        let (status, reply) = move_page(&app, "guide.md", &base, to).await;
        assert_eq!(status, StatusCode::CONFLICT, "{to}: {reply}");
        assert_eq!(history_len(&upstream.dir, BRANCH), 1, "{to}: nothing pushed");
    }
}

#[tokio::test]
async fn a_move_and_its_move_back_redirect_to_the_first_live_hop() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n"), ("a.md", PAGE)]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki);
    let base = base_oid(&app, "a.md").await;
    let (status, reply) = move_page(&app, "a.md", &base, "b.md").await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let (status, reply) = move_page(&app, "b.md", &base, "a.md").await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(
        location(&app, "/b").await,
        (StatusCode::MOVED_PERMANENTLY, Some("/a".to_string()))
    );
    let (status, page) = html(&app, "/a").await;
    assert_eq!(status, StatusCode::OK, "the page at /a wins over its own redirect");
    assert!(page.contains("The original words."), "{page}");
}

#[tokio::test]
async fn new_skips_the_redirect_and_offers_create_this_page() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n"), ("foo.md", PAGE)]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki);
    let base = base_oid(&app, "foo.md").await;
    let (status, reply) = move_page(&app, "foo.md", &base, "bar.md").await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(
        location(&app, "/foo").await,
        (StatusCode::MOVED_PERMANENTLY, Some("/bar".to_string()))
    );
    let (status, page) = html(&app, "/foo?new=Foo").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no 301");
    assert!(page.contains("Create this page"), "{page}");
    assert!(page.contains(r#"data-path="foo.md""#), "{page}");
}

#[tokio::test]
async fn move_push_timeout_is_503_retry_safe_then_retry_is_200_content_present() {
    let upstream = seeded();
    let wiki = upstream.replica("r", Duration::from_secs(2)).await;
    hang_after_receive(&upstream, "r");
    let app = app(&wiki);
    let base = base_oid(&app, "guide.md").await;

    let (status, reply) = move_page(&app, "guide.md", &base, "handbook.md").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{reply}");
    assert_eq!(reply["retry-safe"], true, "{reply}");
    assert_eq!(history_len(&upstream.dir, BRANCH), 2, "the timed-out push landed");

    let (status, reply) = move_page(&app, "guide.md", &base, "handbook.md").await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(
        reply,
        json!({"commit": null, "content-present": true, "url": "/handbook"})
    );
    assert_eq!(history_len(&upstream.dir, BRANCH), 2, "exactly one move commit");
    assert_eq!(
        location(&app, "/guide").await,
        (StatusCode::MOVED_PERMANENTLY, Some("/handbook".to_string())),
        "content-present published the tip with its redirects"
    );
}

#[tokio::test]
async fn bad_moves_are_400_and_commit_nothing() {
    let upstream = seeded();
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki);
    let readme = base_oid(&app, "README.md").await;
    let guide = base_oid(&app, "guide.md").await;
    for (from, base, to) in [
        ("README.md", readme.as_str(), "home.md"),
        ("guide.md", &guide, "guide.md"),
        ("guide.md", &guide, "guide.txt"),
        ("guide.md", &guide, "_riki/guide.md"),
        ("guide.md", "zz", "handbook.md"),
    ] {
        let (status, reply) = move_page(&app, from, base, to).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{from} -> {to}: {reply}");
    }
    let (status, _) = post(&app, "/_riki/api/move", json!({"from": "guide.md", "base-oid": guide})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "missing to");
    assert_eq!(history_len(&upstream.dir, BRANCH), 1);
}

#[tokio::test]
async fn move_and_restore_urls_are_percent_encoded() {
    let upstream = seeded();
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki);
    let base = base_oid(&app, "guide.md").await;
    let (status, reply) = move_page(&app, "guide.md", &base, "docs/a#b.md").await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["url"], "/docs/a%23b");

    let base = base_oid(&app, "docs/a%23b.md").await;
    let (status, reply) = delete(&app, "docs/a#b.md", &base).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let d = reply["commit"].as_str().expect("a commit").to_string();
    let (status, reply) = restore(&app, "docs/a#b.md", &d).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["url"], "/docs/a%23b");
}

#[tokio::test]
async fn restore_under_a_file_that_took_a_folder_name_is_a_409() {
    let upstream = Upstream::new();
    upstream.push(&[("README.md", "# home\n"), ("a/x.md", PAGE)]);
    let wiki = upstream.replica("r", TIMEOUT).await;
    let app = app(&wiki);
    let base = base_oid(&app, "a/x.md").await;
    let (status, reply) = delete(&app, "a/x.md", &base).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let d = reply["commit"].as_str().expect("a commit").to_string();
    let laptop = upstream.push(&[("a", "not a folder\n")]);
    let (status, reply) = restore(&app, "a/x.md", &d).await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(head(&upstream.dir, BRANCH), Some(laptop), "nothing pushed");
}
