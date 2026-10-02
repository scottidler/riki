use std::path::{Path, PathBuf};
use std::time::Duration;

use tempfile::TempDir;

use super::*;
use crate::store::GOOD_REF;
use crate::testing::{commit, commit_files, file_url, init_upstream};

const BRANCH: &str = "main";

struct Fixture {
    tmp: TempDir,
    upstream: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = TempDir::new().expect("tmp");
        let upstream = tmp.path().join("upstream.git");
        init_upstream(&upstream);
        Self { tmp, upstream }
    }

    fn config(&self) -> StoreConfig {
        StoreConfig {
            remote: file_url(&self.upstream),
            branch: BRANCH.to_string(),
            cache_dir: self.tmp.path().join("content.git"),
            timeout: Duration::from_secs(30),
        }
    }

    fn push(&self, files: &[(&str, &str)]) -> Oid {
        commit_files(&self.upstream, BRANCH, files, "push")
    }

    fn cache(&self) -> &Path {
        self.tmp.path()
    }
}

fn good_ref(cache_dir: &Path) -> Option<Oid> {
    git2::Repository::open_bare(cache_dir.join("content.git"))
        .expect("cache repo")
        .refname_to_id(GOOD_REF)
        .ok()
}

#[tokio::test]
async fn commit_pushed_upstream_is_readable_after_one_poll() {
    let fx = Fixture::new();
    let first = fx.push(&[("README.md", "# one\n")]);
    let wiki = Wiki::open(&fx.config()).await.expect("open");
    assert!(wiki.good().is_none(), "nothing fetched yet");
    assert_eq!(wiki.poll().await.expect("poll"), PollOutcome::Published(first));

    let second = fx.push(&[("a/b.md", "# laptop push\n")]);
    assert_eq!(wiki.poll().await.expect("poll"), PollOutcome::Published(second));
    let good = wiki.good().expect("good");
    assert_eq!(good.commit(), second);
    let file = good.nav.file_for_url("a/b").expect("indexed");
    let body = wiki.store().read_blob(good.commit(), file).await.expect("read");
    assert_eq!(body.as_deref(), Some(b"# laptop push\n".as_slice()));
    assert_eq!(good_ref(fx.cache()), Some(second));
    assert_eq!(wiki.status_error(), None);
}

#[tokio::test]
async fn poll_without_a_new_tip_is_unchanged() {
    let fx = Fixture::new();
    fx.push(&[("README.md", "x\n")]);
    let wiki = Wiki::open(&fx.config()).await.expect("open");
    wiki.poll().await.expect("poll");
    assert_eq!(wiki.poll().await.expect("poll"), PollOutcome::Unchanged);
}

#[tokio::test]
async fn tip_with_index_error_leaves_good_unchanged_and_degrades_status() {
    let fx = Fixture::new();
    let good = fx.push(&[("README.md", "x\n"), ("a/b.md", "file\n")]);
    let wiki = Wiki::open(&fx.config()).await.expect("open");
    wiki.poll().await.expect("poll");
    let bad = fx.push(&[("a/b/README.md", "dir readme\n")]);

    assert_eq!(wiki.poll().await.expect("poll"), PollOutcome::Refused(bad));
    assert_eq!(good_ref(fx.cache()), Some(good), "refs/riki/good did not move");
    assert_eq!(
        wiki.good().expect("good").commit(),
        good,
        "in-memory index did not swap"
    );
    let rejected = wiki.rejected().expect("rejected");
    assert_eq!(rejected.commit, bad);
    let error = wiki.status_error().expect("degraded");
    assert!(error.contains(&bad.to_string()), "{error}");
    assert!(error.contains("map to /a/b"), "{error}");
}

#[tokio::test]
async fn reserved_name_on_tip_is_refused() {
    let fx = Fixture::new();
    let good = fx.push(&[("README.md", "x\n")]);
    let wiki = Wiki::open(&fx.config()).await.expect("open");
    wiki.poll().await.expect("poll");
    let bad = fx.push(&[("status.md", "shadow\n")]);
    assert_eq!(wiki.poll().await.expect("poll"), PollOutcome::Refused(bad));
    assert_eq!(wiki.good().expect("good").commit(), good);
    assert!(wiki.status_error().expect("degraded").contains("reserved name /status"));
}

#[tokio::test]
async fn restart_after_an_invalid_tip_serves_refs_riki_good() {
    let fx = Fixture::new();
    let good = fx.push(&[("README.md", "good\n")]);
    {
        let wiki = Wiki::open(&fx.config()).await.expect("open");
        wiki.poll().await.expect("poll");
        fx.push(&[("README.md", "bad\n"), ("health.md", "shadow\n")]);
        assert!(matches!(wiki.poll().await.expect("poll"), PollOutcome::Refused(_)));
    }
    // The clone's tip is the invalid commit now; a restart must not serve it.
    let restarted = Wiki::open(&fx.config()).await.expect("reopen");
    let served = restarted.good().expect("good");
    assert_eq!(served.commit(), good);
    let body = restarted
        .store()
        .read_blob(served.commit(), "README.md")
        .await
        .expect("read");
    assert_eq!(body.as_deref(), Some(b"good\n".as_slice()));
    assert_ne!(restarted.store().tip().await.expect("tip"), Some(good));
    // Its first poll sees the same invalid tip and reports it.
    assert!(matches!(restarted.poll().await.expect("poll"), PollOutcome::Refused(_)));
    assert!(restarted.status_error().is_some());
}

#[tokio::test]
async fn first_run_without_good_ref_starts_from_the_tip() {
    let fx = Fixture::new();
    let tip = fx.push(&[("README.md", "x\n")]);
    let store = GitStore::open(&fx.config()).await.expect("store");
    store.fetch(&store.lock().await).await.expect("fetch");
    drop(store);
    let wiki = Wiki::open(&fx.config()).await.expect("open");
    assert_eq!(wiki.good().expect("good").commit(), tip);
    assert_eq!(good_ref(fx.cache()), Some(tip));
}

#[tokio::test]
async fn upstream_unreachable_then_recovered_clears_the_error() {
    let fx = Fixture::new();
    let good = fx.push(&[("README.md", "x\n")]);
    let wiki = Wiki::open(&fx.config()).await.expect("open");
    wiki.poll().await.expect("poll");
    assert_eq!(wiki.status_error(), None);

    let parked = fx.tmp.path().join("parked.git");
    std::fs::rename(&fx.upstream, &parked).expect("take upstream away");
    assert_eq!(wiki.poll().await.expect("poll"), PollOutcome::Unreachable);
    let down = wiki.unreachable().expect("unreachable");
    let error = wiki.status_error().expect("degraded");
    assert!(error.starts_with("upstream unreachable since "), "{error}");
    assert_eq!(wiki.good().expect("still serving").commit(), good);
    assert_eq!(wiki.poll().await.expect("poll"), PollOutcome::Unreachable);
    assert_eq!(
        wiki.unreachable().expect("still down").since,
        down.since,
        "since is the first failure"
    );

    std::fs::rename(&parked, &fx.upstream).expect("bring upstream back");
    assert_eq!(wiki.poll().await.expect("poll"), PollOutcome::Unchanged);
    assert_eq!(wiki.unreachable(), None);
    assert_eq!(wiki.status_error(), None);
}

#[tokio::test]
async fn a_fixed_tip_publishes_and_clears_the_rejection() {
    let fx = Fixture::new();
    fx.push(&[("README.md", "x\n")]);
    let wiki = Wiki::open(&fx.config()).await.expect("open");
    wiki.poll().await.expect("poll");
    fx.push(&[("version.md", "shadow\n")]);
    wiki.poll().await.expect("poll");
    assert!(wiki.rejected().is_some());
    let fixed = commit(&fx.upstream, BRANCH, &[("version.md", None)], "revert");
    assert_eq!(wiki.poll().await.expect("poll"), PollOutcome::Published(fixed));
    assert_eq!(wiki.rejected(), None);
    assert_eq!(wiki.status_error(), None);
}

#[tokio::test]
async fn index_is_cached_by_commit() {
    let fx = Fixture::new();
    let tip = fx.push(&[("README.md", "x\n")]);
    let wiki = Wiki::open(&fx.config()).await.expect("open");
    wiki.poll().await.expect("poll");
    let first = wiki.index(tip).await.expect("index");
    let second = wiki.index(tip).await.expect("index");
    assert!(Arc::ptr_eq(&first, &second));
}
