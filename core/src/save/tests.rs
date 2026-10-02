use std::path::PathBuf;
use std::time::Duration;

use tempfile::TempDir;

use super::*;
use crate::store::StoreConfig;
use crate::testing::{commit_files, file_at, file_url, head, history_len, init_upstream};

const BRANCH: &str = "main";

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

fn request(path: &str, base_oid: Option<Oid>, body: &str) -> SaveRequest {
    SaveRequest {
        path: path.to_string(),
        base_oid,
        body: body.to_string(),
        message: None,
    }
}

async fn oid_of(wiki: &Wiki, path: &str) -> Option<Oid> {
    let tip = wiki.store().tip().await.expect("tip").expect("a tip");
    wiki.store().blob_at(tip, path).await.expect("read").map(|(oid, _)| oid)
}

#[tokio::test]
async fn a_new_page_is_committed_pushed_and_published() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    let outcome = save(&wiki, &settings(), &alice(), &request("a/new.md", None, "# New"))
        .await
        .expect("save");
    let SaveOutcome::Saved { commit } = outcome else {
        panic!("expected Saved, got {outcome:?}");
    };
    assert_eq!(head(&fx.upstream, BRANCH), Some(commit));
    assert_eq!(file_at(&fx.upstream, BRANCH, "a/new.md"), Some(b"# New\n".to_vec()));
    assert_eq!(wiki.good().expect("good").commit(), commit);
    assert_eq!(wiki.store().tip().await.expect("tip"), Some(commit));
}

#[tokio::test]
async fn saving_the_stored_bytes_is_unchanged_and_commits_nothing() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    let base = oid_of(&wiki, "README.md").await;
    let outcome = save(&wiki, &settings(), &alice(), &request("README.md", base, "# home"))
        .await
        .expect("save");
    assert_eq!(outcome, SaveOutcome::Unchanged);
    assert_eq!(history_len(&fx.upstream, BRANCH), 1);
}

#[tokio::test]
async fn a_bad_path_or_message_is_a_bad_request() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    for path in ["a.txt", "../x.md", "status.md", ".hidden.md"] {
        let outcome = save(&wiki, &settings(), &alice(), &request(path, None, "x"))
            .await
            .expect("save");
        assert!(matches!(outcome, SaveOutcome::BadRequest(_)), "{path}: {outcome:?}");
    }
    let mut two_lines = request("x.md", None, "x");
    two_lines.message = Some("one\ntwo".to_string());
    let outcome = save(&wiki, &settings(), &alice(), &two_lines).await.expect("save");
    assert!(matches!(outcome, SaveOutcome::BadRequest(_)), "{outcome:?}");
    assert_eq!(history_len(&fx.upstream, BRANCH), 1);
}

#[test]
fn commit_message_defaults_and_trims() {
    let mut req = request("a/b.md", None, "");
    assert_eq!(commit_message(&req), Ok("riki: edit a/b.md".to_string()));
    req.message = Some("   ".to_string());
    assert_eq!(commit_message(&req), Ok("riki: edit a/b.md".to_string()));
    req.message = Some(" fix typo ".to_string());
    assert_eq!(commit_message(&req), Ok("fix typo".to_string()));
    req.message = Some("a\r\nb".to_string());
    assert!(commit_message(&req).is_err());
}

#[tokio::test]
async fn a_custom_message_is_used() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    let mut req = request("x.md", None, "x");
    req.message = Some("add x".to_string());
    let outcome = save(&wiki, &settings(), &alice(), &req).await.expect("save");
    let SaveOutcome::Saved { commit } = outcome else {
        panic!("{outcome:?}");
    };
    let repo = git2::Repository::open_bare(&fx.upstream).expect("repo");
    let commit = repo.find_commit(commit).expect("commit");
    assert_eq!(commit.message().ok(), Some("add x"));
    assert_eq!(commit.author().email().ok(), Some("alice@example.com"));
    assert_eq!(commit.committer().name().ok(), Some("riki"));
}

#[tokio::test]
async fn unchanged_on_a_tip_that_refuses_publish_is_an_index_conflict_not_a_200() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    let good = wiki.good().expect("good").commit();
    let base = oid_of(&wiki, "README.md").await;
    commit_files(&fx.upstream, BRANCH, &[("status.md", "x\n")], "reserved name");
    let outcome = save(&wiki, &settings(), &alice(), &request("README.md", base, "# home"))
        .await
        .expect("save");
    let SaveOutcome::IndexConflict { errors } = outcome else {
        panic!("expected IndexConflict, got {outcome:?}");
    };
    assert!(errors.contains("reserved name /status"), "{errors}");
    assert_eq!(wiki.good().expect("good").commit(), good, "good tip did not move");
}

#[tokio::test]
async fn content_present_on_a_tip_that_refuses_publish_is_an_index_conflict_not_a_200() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    let good = wiki.good().expect("good").commit();
    let base = oid_of(&wiki, "README.md").await;
    commit_files(
        &fx.upstream,
        BRANCH,
        &[("README.md", "# new\n"), ("status.md", "x\n")],
        "same edit plus a reserved name",
    );
    let outcome = save(&wiki, &settings(), &alice(), &request("README.md", base, "# new"))
        .await
        .expect("save");
    let SaveOutcome::IndexConflict { errors } = outcome else {
        panic!("expected IndexConflict, got {outcome:?}");
    };
    assert!(errors.contains("reserved name /status"), "{errors}");
    assert_eq!(wiki.good().expect("good").commit(), good, "good tip did not move");
}

#[tokio::test]
async fn a_save_observed_outage_and_recovery_update_upstream_health() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    assert_eq!(wiki.unreachable(), None);

    let parked = fx.tmp.path().join("parked.git");
    std::fs::rename(&fx.upstream, &parked).expect("take upstream away");
    let outcome = save(&wiki, &settings(), &alice(), &request("x.md", None, "x"))
        .await
        .expect("save");
    assert!(matches!(outcome, SaveOutcome::FetchFailed(_)), "{outcome:?}");
    assert!(
        wiki.unreachable().is_some(),
        "a save's failed fetch marks upstream unreachable"
    );
    let error = wiki.status_error().expect("degraded");
    assert!(error.starts_with("upstream unreachable since "), "{error}");

    std::fs::rename(&parked, &fx.upstream).expect("bring upstream back");
    let outcome = save(&wiki, &settings(), &alice(), &request("x.md", None, "x"))
        .await
        .expect("save");
    assert!(matches!(outcome, SaveOutcome::Saved { .. }), "{outcome:?}");
    assert_eq!(wiki.unreachable(), None, "a save's successful fetch clears the outage");
    assert_eq!(wiki.status_error(), None);
}
