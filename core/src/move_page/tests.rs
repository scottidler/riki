use std::path::PathBuf;
use std::time::Duration;

use tempfile::TempDir;

use super::*;
use crate::store::StoreConfig;
use crate::testing::{commit_files, file_at, file_url, head, history_len, init_upstream};

const BRANCH: &str = "main";
const PAGE: &str = "# Guide\n\nwords\n";

struct Fixture {
    tmp: TempDir,
    upstream: PathBuf,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let tmp = TempDir::new().expect("tmp");
        let upstream = tmp.path().join("upstream.git");
        init_upstream(&upstream);
        commit_files(&upstream, BRANCH, files, "seed");
        Self { tmp, upstream }
    }

    async fn wiki(&self) -> Wiki {
        let wiki = Wiki::open(&StoreConfig {
            remote: file_url(&self.upstream),
            branch: BRANCH.to_string(),
            cache_dir: self.tmp.path().join("content.git"),
            timeout: Duration::from_secs(30),
        })
        .await
        .expect("open");
        wiki.poll().await.expect("poll");
        wiki
    }

    fn message(&self, commit: Oid) -> String {
        let repo = git2::Repository::open_bare(&self.upstream).expect("repo");
        let commit = repo.find_commit(commit).expect("commit");
        commit.message().expect("utf-8").to_string()
    }
}

fn settings() -> SaveSettings {
    SaveSettings {
        committer: Signer {
            name: "riki".to_string(),
            email: "riki@localhost".to_string(),
        },
        push_retries: 1,
    }
}

fn alice() -> Signer {
    Signer {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
    }
}

async fn oid_of(wiki: &Wiki, path: &str) -> Oid {
    let tip = wiki.store().tip().await.expect("tip").expect("a tip");
    wiki.store().entry_at(tip, path).await.expect("read").expect("present")
}

fn request(from: &str, base_oid: Oid, to: &str) -> MoveRequest {
    MoveRequest {
        from: from.to_string(),
        base_oid,
        to: to.to_string(),
        message: None,
    }
}

async fn run(wiki: &Wiki, request: &MoveRequest) -> WriteOutcome<OpAnswer> {
    move_page(wiki, &settings(), &alice(), request).await.expect("move")
}

async fn conflict(fx: &Fixture, wiki: &Wiki, from: &str, to: &str) -> String {
    let before = head(&fx.upstream, BRANCH);
    let base = oid_of(wiki, from).await;
    let outcome = run(wiki, &request(from, base, to)).await;
    assert_eq!(head(&fx.upstream, BRANCH), before, "{from} -> {to}: nothing pushed");
    match outcome {
        WriteOutcome::Op(OpAnswer::Conflict(reason)) => reason,
        outcome => panic!("{from} -> {to}: expected a conflict, got {outcome:?}"),
    }
}

#[tokio::test]
async fn a_move_is_one_commit_with_the_same_blob_at_the_new_path() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("a/guide.md", PAGE)]);
    let wiki = fx.wiki().await;
    let base = oid_of(&wiki, "a/guide.md").await;
    let outcome = run(&wiki, &request("a/guide.md", base, "b/guide.md")).await;
    let WriteOutcome::Pushed { commit } = outcome else {
        panic!("expected Pushed, got {outcome:?}");
    };
    assert_eq!(head(&fx.upstream, BRANCH), Some(commit));
    assert_eq!(history_len(&fx.upstream, BRANCH), 2);
    assert_eq!(file_at(&fx.upstream, BRANCH, "a/guide.md"), None);
    assert_eq!(
        file_at(&fx.upstream, BRANCH, "b/guide.md"),
        Some(PAGE.as_bytes().to_vec())
    );
    assert_eq!(oid_of(&wiki, "b/guide.md").await, base, "the same blob");
    assert_eq!(fx.message(commit), "riki: move a/guide.md -> b/guide.md");
    let good = wiki.good().expect("good");
    assert_eq!(good.commit(), commit, "published");
    assert_eq!(good.redirects.entries().collect::<Vec<_>>(), [("a/guide", "b/guide")]);
}

#[tokio::test]
async fn a_user_message_replaces_the_default() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("x.md", PAGE)]);
    let wiki = fx.wiki().await;
    let base = oid_of(&wiki, "x.md").await;
    let mut req = request("x.md", base, "y.md");
    req.message = Some("tidy up".to_string());
    let WriteOutcome::Pushed { commit } = run(&wiki, &req).await else {
        panic!("expected Pushed");
    };
    assert_eq!(fx.message(commit), "tidy up");
}

#[tokio::test]
async fn a_retried_move_is_content_present_and_commits_nothing() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("x.md", PAGE)]);
    let wiki = fx.wiki().await;
    let base = oid_of(&wiki, "x.md").await;
    let first = run(&wiki, &request("x.md", base, "y.md")).await;
    assert!(matches!(first, WriteOutcome::Pushed { .. }), "{first:?}");
    let pushed = head(&fx.upstream, BRANCH);
    let again = run(&wiki, &request("x.md", base, "y.md")).await;
    assert_eq!(again, WriteOutcome::Op(OpAnswer::ContentPresent));
    assert_eq!(head(&fx.upstream, BRANCH), pushed, "no commit");
}

#[tokio::test]
async fn a_stale_base_oid_is_a_conflict_and_nothing_pushed() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("x.md", PAGE)]);
    let wiki = fx.wiki().await;
    let base = oid_of(&wiki, "x.md").await;
    let laptop = commit_files(&fx.upstream, BRANCH, &[("x.md", "# X edited\n")], "laptop");
    let outcome = run(&wiki, &request("x.md", base, "y.md")).await;
    assert!(
        matches!(&outcome, WriteOutcome::Op(OpAnswer::Conflict(reason)) if reason.contains("changed")),
        "{outcome:?}"
    );
    assert_eq!(head(&fx.upstream, BRANCH), Some(laptop), "nothing pushed");
    assert_eq!(
        wiki.good().expect("good").commit(),
        laptop,
        "the conflict publishes the tip"
    );
}

#[tokio::test]
async fn a_move_from_a_vanished_page_is_a_conflict() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("x.md", PAGE)]);
    let wiki = fx.wiki().await;
    let base = oid_of(&wiki, "x.md").await;
    crate::testing::commit(&fx.upstream, BRANCH, &[("x.md", None)], "laptop rm");
    let outcome = run(&wiki, &request("x.md", base, "y.md")).await;
    assert!(
        matches!(&outcome, WriteOutcome::Op(OpAnswer::Conflict(reason)) if reason.contains("no longer exists")),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn a_move_onto_an_existing_page_or_folder_is_a_conflict() {
    let fx = Fixture::new(&[
        ("README.md", "# home\n"),
        ("x.md", PAGE),
        ("y.md", "# Y\n"),
        ("d.md/z.md", "# Z\n"),
    ]);
    let wiki = fx.wiki().await;
    assert!(
        conflict(&fx, &wiki, "x.md", "y.md")
            .await
            .contains("y.md already exists")
    );
    assert!(
        conflict(&fx, &wiki, "x.md", "d.md")
            .await
            .contains("d.md already exists")
    );
}

#[tokio::test]
async fn a_move_into_a_readme_collision_is_a_conflict() {
    let fx = Fixture::new(&[
        ("README.md", "# home\n"),
        ("x.md", PAGE),
        ("guide/README.md", "# Guide folder\n"),
        ("notes.md", "# Notes\n"),
    ]);
    let wiki = fx.wiki().await;
    let reason = conflict(&fx, &wiki, "x.md", "guide.md").await;
    assert!(reason.contains("guide/README.md already serves /guide"), "{reason}");
    let reason = conflict(&fx, &wiki, "x.md", "notes/README.md").await;
    assert!(reason.contains("notes.md already serves /notes"), "{reason}");
}

#[tokio::test]
async fn a_move_under_a_file_is_a_conflict() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("x.md", PAGE), ("y.md", "# Y\n")]);
    let wiki = fx.wiki().await;
    let reason = conflict(&fx, &wiki, "x.md", "y.md/x.md").await;
    assert!(reason.contains("y.md is a file"), "{reason}");
}

#[tokio::test]
async fn a_page_can_move_into_its_own_folder_readme() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("a.md", PAGE), ("a/b.md", "# B\n")]);
    let wiki = fx.wiki().await;
    let base = oid_of(&wiki, "a.md").await;
    let outcome = run(&wiki, &request("a.md", base, "a/README.md")).await;
    assert!(matches!(outcome, WriteOutcome::Pushed { .. }), "{outcome:?}");
    assert_eq!(
        wiki.good().expect("good").redirects.entries().count(),
        0,
        "same URL, no redirect"
    );
}

#[tokio::test]
async fn bad_requests_never_reach_the_driver() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("x.md", PAGE), ("guide/README.md", "# G\n")]);
    let wiki = fx.wiki().await;
    let base = oid_of(&wiki, "x.md").await;
    let readme = oid_of(&wiki, "README.md").await;
    let folder = oid_of(&wiki, "guide/README.md").await;
    let mut multiline = request("x.md", base, "y.md");
    multiline.message = Some("one\ntwo".to_string());
    for (req, why) in [
        (request("README.md", readme, "home.md"), "folder move"),
        (request("guide/README.md", folder, "g.md"), "folder move"),
        (request("x.md", base, "x.md"), "already at"),
        (request("x.md", base, "x.txt"), ""),
        (request("x.md", base, "../x.md"), ""),
        (request("x.md", base, "status.md"), ""),
        (multiline, "one line"),
    ] {
        let outcome = run(&wiki, &req).await;
        let WriteOutcome::Op(OpAnswer::BadRequest(message)) = &outcome else {
            panic!("{req:?}: expected BadRequest, got {outcome:?}");
        };
        assert!(message.contains(why), "{req:?}: {message}");
    }
    assert_eq!(history_len(&fx.upstream, BRANCH), 1, "nothing pushed");
}

#[test]
fn twins_are_the_two_files_of_one_url() {
    assert_eq!(twin("a/b.md").as_deref(), Some("a/b/README.md"));
    assert_eq!(twin("a/b/README.md").as_deref(), Some("a/b.md"));
    assert_eq!(twin("top.md").as_deref(), Some("top/README.md"));
    assert_eq!(twin("README.md"), None);
}
