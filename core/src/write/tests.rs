use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use tempfile::TempDir;

use super::*;
use crate::store::{FileCommit, StoreConfig};
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

fn settings(push_retries: u32) -> SaveSettings {
    SaveSettings {
        committer: Signer {
            name: "riki".to_string(),
            email: "riki@localhost".to_string(),
        },
        push_retries,
    }
}

fn alice() -> Signer {
    Signer {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Commit { path: &'static str, contents: &'static str },
    NoCommit,
    Conflict,
    Refused,
}

/// An op that answers the same way on every tip, optionally racing a push into upstream on its
/// first call so the driver's own push is rejected as non-fast-forward.
struct TestOp {
    answer: Answer,
    race_upstream: Option<PathBuf>,
    calls: AtomicU32,
}

impl TestOp {
    fn new(answer: Answer) -> Self {
        Self {
            answer,
            race_upstream: None,
            calls: AtomicU32::new(0),
        }
    }

    fn racing(answer: Answer, upstream: &Path) -> Self {
        Self {
            race_upstream: Some(upstream.to_path_buf()),
            ..Self::new(answer)
        }
    }
}

impl WriteOp for TestOp {
    type Outcome = &'static str;

    async fn check_and_build(&self, at: &Fetched<'_>) -> Result<Check<&'static str>, StoreError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(match self.answer {
            Answer::Commit { path, contents } => {
                let commit = at
                    .store
                    .commit_file(FileCommit {
                        parent: at.tip,
                        path,
                        contents: contents.as_bytes(),
                        author: at.author,
                        committer: at.committer,
                        message: "test op",
                    })
                    .await?;
                if call == 0
                    && let Some(upstream) = &self.race_upstream
                {
                    commit_files(upstream, BRANCH, &[("race.md", "laptop\n")], "race");
                }
                Check::Committed(commit)
            }
            Answer::NoCommit => Check::NoCommit("no commit"),
            Answer::Conflict => Check::Conflict("conflict"),
            Answer::Refused => Check::Refused("refused"),
        })
    }
}

#[tokio::test]
async fn a_committed_op_is_pushed_and_published() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    let op = TestOp::new(Answer::Commit {
        path: "a.md",
        contents: "# a\n",
    });
    let outcome = run(&wiki, &settings(1), &alice(), &op).await.expect("run");
    let WriteOutcome::Pushed { commit } = outcome else {
        panic!("expected Pushed, got {outcome:?}");
    };
    assert_eq!(head(&fx.upstream, BRANCH), Some(commit));
    assert_eq!(file_at(&fx.upstream, BRANCH, "a.md"), Some(b"# a\n".to_vec()));
    assert_eq!(wiki.good().expect("good").commit(), commit);
    assert_eq!(wiki.store().tip().await.expect("tip"), Some(commit));
}

#[tokio::test]
async fn a_commit_whose_index_has_errors_is_not_pushed() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    let op = TestOp::new(Answer::Commit {
        path: "status.md",
        contents: "x\n",
    });
    let outcome = run(&wiki, &settings(1), &alice(), &op).await.expect("run");
    let WriteOutcome::IndexConflict { errors } = outcome else {
        panic!("expected IndexConflict, got {outcome:?}");
    };
    assert!(errors.contains("reserved name /status"), "{errors}");
    assert_eq!(history_len(&fx.upstream, BRANCH), 1);
}

#[tokio::test]
async fn a_non_fast_forward_push_refetches_and_rebuilds_on_the_new_tip() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    let op = TestOp::racing(
        Answer::Commit {
            path: "a.md",
            contents: "# a\n",
        },
        &fx.upstream,
    );
    let outcome = run(&wiki, &settings(1), &alice(), &op).await.expect("run");
    assert!(matches!(outcome, WriteOutcome::Pushed { .. }), "{outcome:?}");
    assert_eq!(
        op.calls.load(Ordering::SeqCst),
        2,
        "check-and-build reran on the new tip"
    );
    assert_eq!(history_len(&fx.upstream, BRANCH), 3);
    assert_eq!(file_at(&fx.upstream, BRANCH, "race.md"), Some(b"laptop\n".to_vec()));
    assert_eq!(file_at(&fx.upstream, BRANCH, "a.md"), Some(b"# a\n".to_vec()));
}

#[tokio::test]
async fn retries_exhausted_reports_every_attempt() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    let op = TestOp::racing(
        Answer::Commit {
            path: "a.md",
            contents: "# a\n",
        },
        &fx.upstream,
    );
    let outcome = run(&wiki, &settings(0), &alice(), &op).await.expect("run");
    let WriteOutcome::RetriesExhausted { attempts, .. } = outcome else {
        panic!("expected RetriesExhausted, got {outcome:?}");
    };
    assert_eq!(attempts, 1);
    assert_eq!(file_at(&fx.upstream, BRANCH, "a.md"), None);
}

#[tokio::test]
async fn no_commit_publishes_the_fetched_tip_first() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    let upstream_tip = commit_files(&fx.upstream, BRANCH, &[("b.md", "# b\n")], "laptop");
    let outcome = run(&wiki, &settings(1), &alice(), &TestOp::new(Answer::NoCommit))
        .await
        .expect("run");
    assert_eq!(outcome, WriteOutcome::Op("no commit"));
    assert_eq!(wiki.good().expect("good").commit(), upstream_tip);
}

#[tokio::test]
async fn no_commit_on_a_tip_that_refuses_publish_is_an_index_conflict() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    let good = wiki.good().expect("good").commit();
    commit_files(&fx.upstream, BRANCH, &[("status.md", "x\n")], "reserved name");
    let outcome = run(&wiki, &settings(1), &alice(), &TestOp::new(Answer::NoCommit))
        .await
        .expect("run");
    let WriteOutcome::IndexConflict { errors } = outcome else {
        panic!("expected IndexConflict, got {outcome:?}");
    };
    assert!(errors.contains("reserved name /status"), "{errors}");
    assert_eq!(wiki.good().expect("good").commit(), good, "good tip did not move");
}

#[tokio::test]
async fn a_conflict_publishes_the_tip_and_answers_the_conflict_even_when_refused() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    let upstream_tip = commit_files(&fx.upstream, BRANCH, &[("b.md", "# b\n")], "laptop");
    let outcome = run(&wiki, &settings(1), &alice(), &TestOp::new(Answer::Conflict))
        .await
        .expect("run");
    assert_eq!(outcome, WriteOutcome::Op("conflict"));
    assert_eq!(wiki.good().expect("good").commit(), upstream_tip);

    commit_files(&fx.upstream, BRANCH, &[("status.md", "x\n")], "reserved name");
    let outcome = run(&wiki, &settings(1), &alice(), &TestOp::new(Answer::Conflict))
        .await
        .expect("run");
    assert_eq!(outcome, WriteOutcome::Op("conflict"));
    assert_eq!(
        wiki.good().expect("good").commit(),
        upstream_tip,
        "good tip did not move"
    );
}

#[tokio::test]
async fn a_refused_op_publishes_nothing() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    let good = wiki.good().expect("good").commit();
    commit_files(&fx.upstream, BRANCH, &[("b.md", "# b\n")], "laptop");
    let outcome = run(&wiki, &settings(1), &alice(), &TestOp::new(Answer::Refused))
        .await
        .expect("run");
    assert_eq!(outcome, WriteOutcome::Op("refused"));
    assert_eq!(wiki.good().expect("good").commit(), good);
}

#[tokio::test]
async fn a_failed_fetch_commits_nothing() {
    let fx = Fixture::new(&[("README.md", "# home\n")]);
    let wiki = fx.wiki().await;
    std::fs::rename(&fx.upstream, fx.tmp.path().join("parked.git")).expect("take upstream away");
    let op = TestOp::new(Answer::Commit {
        path: "a.md",
        contents: "# a\n",
    });
    let outcome = run(&wiki, &settings(1), &alice(), &op).await.expect("run");
    assert!(matches!(outcome, WriteOutcome::FetchFailed(_)), "{outcome:?}");
    assert_eq!(op.calls.load(Ordering::SeqCst), 0, "no check without a fresh tip");
}
