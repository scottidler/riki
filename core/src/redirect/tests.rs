use std::path::PathBuf;
use std::time::Duration;

use tempfile::TempDir;

use super::*;
use crate::store::StoreConfig;
use crate::testing::{commit, commit_files, file_url, init_upstream};
use crate::wiki::Wiki;

const BRANCH: &str = "main";

fn map(pairs: &[(&str, &str)]) -> Redirects {
    Redirects {
        map: pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        walked: None,
    }
}

fn rename(from: &str, to: &str) -> Rename {
    Rename {
        from: from.to_string(),
        to: to.to_string(),
    }
}

#[test]
fn resolve_stops_at_the_first_live_hop() {
    let redirects = map(&[("a", "b"), ("b", "c"), ("c", "d")]);
    assert_eq!(redirects.resolve("a", |url| url == "c" || url == "d"), Some("c"));
    assert_eq!(redirects.resolve("a", |url| url == "d"), Some("d"));
    assert_eq!(redirects.resolve("b", |url| url == "d"), Some("d"));
}

#[test]
fn resolve_is_none_when_the_chain_ends_dead_or_unknown() {
    let redirects = map(&[("a", "b")]);
    assert_eq!(redirects.resolve("a", |_| false), None, "dead end");
    assert_eq!(redirects.resolve("zzz", |_| true), None, "not moved");
    assert_eq!(Redirects::default().resolve("a", |_| true), None);
}

#[test]
fn resolve_gives_up_on_a_cycle_with_no_live_hop() {
    let redirects = map(&[("a", "b"), ("b", "a")]);
    assert_eq!(redirects.resolve("a", |_| false), None);
    assert_eq!(redirects.resolve("b", |url| url == "a"), Some("a"), "moved back");
}

#[test]
fn apply_maps_page_urls_and_skips_the_rest() {
    let mut redirects = Redirects::default();
    redirects.apply(&[
        rename("a/x.md", "b/x.md"),
        rename("img.png", "assets/img.png"),
        rename("notes.txt", "notes.md"),
        rename("guide.md", "guide/README.md"),
        rename("old/README.md", "new/README.md"),
        rename("top.md", "README.md"),
    ]);
    assert_eq!(
        redirects.entries().collect::<Vec<_>>(),
        [("a/x", "b/x"), ("old", "new"), ("top", "")]
    );
}

#[test]
fn a_later_rename_of_the_same_url_wins() {
    let mut redirects = Redirects::default();
    redirects.apply(&[rename("a.md", "b.md"), rename("a.md", "c.md")]);
    assert_eq!(redirects.entries().collect::<Vec<_>>(), [("a", "c")]);
}

struct Repo {
    tmp: TempDir,
    upstream: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let tmp = TempDir::new().expect("tmp");
        let upstream = tmp.path().join("upstream.git");
        init_upstream(&upstream);
        Self { tmp, upstream }
    }

    /// A `git mv` on a laptop: remove `from`, add the same bytes at `to`, one commit.
    fn mv(&self, from: &str, to: &str, bytes: &[u8]) -> Oid {
        commit(
            &self.upstream,
            BRANCH,
            &[(from, None), (to, Some(bytes))],
            &format!("mv {from} {to}"),
        )
    }

    async fn store(&self, name: &str) -> GitStore {
        let store = GitStore::open(&self.config(name)).await.expect("store");
        let guard = store.lock().await;
        store.fetch(&guard).await.expect("fetch");
        drop(guard);
        store
    }

    fn config(&self, name: &str) -> StoreConfig {
        StoreConfig {
            remote: file_url(&self.upstream),
            branch: BRANCH.to_string(),
            cache_dir: self.tmp.path().join(format!("{name}.git")),
            timeout: Duration::from_secs(30),
        }
    }
}

async fn cold(store: &GitStore, commit: Oid) -> Redirects {
    Redirects::build(store, None, commit).await.expect("cold walk")
}

#[tokio::test]
async fn a_full_walk_finds_every_exact_page_rename_on_the_first_parent_chain() {
    let repo = Repo::new();
    commit_files(
        &repo.upstream,
        BRANCH,
        &[("README.md", "# home\n"), ("a.md", "# A\n")],
        "seed",
    );
    repo.mv("a.md", "b.md", b"# A\n");
    commit_files(&repo.upstream, BRANCH, &[("c.md", "# C\n")], "add c");
    commit(
        &repo.upstream,
        BRANCH,
        &[("c.md", None), ("d/c.md", Some(b"# C edited\n"))],
        "move and edit",
    );
    let tip = repo.mv("b.md", "e/b.md", b"# A\n");
    let store = repo.store("r").await;
    let redirects = cold(&store, tip).await;
    assert_eq!(redirects.walked(), Some(tip));
    assert_eq!(
        redirects.entries().collect::<Vec<_>>(),
        [("a", "b"), ("b", "e/b")],
        "a move that also edits gets no redirect"
    );
}

#[tokio::test]
async fn an_incremental_walk_equals_a_cold_full_walk() {
    let repo = Repo::new();
    commit_files(
        &repo.upstream,
        BRANCH,
        &[("README.md", "# home\n"), ("a.md", "# A\n")],
        "seed",
    );
    let first = repo.mv("a.md", "b.md", b"# A\n");
    let store = repo.store("r").await;
    let before = cold(&store, first).await;

    commit_files(&repo.upstream, BRANCH, &[("x.md", "# X\n")], "add x");
    repo.mv("x.md", "y/x.md", b"# X\n");
    let tip = repo.mv("b.md", "a.md", b"# A\n");
    let guard = store.lock().await;
    store.fetch(&guard).await.expect("fetch");
    drop(guard);

    let incremental = Redirects::build(&store, Some(&before), tip).await.expect("incremental");
    assert_eq!(incremental, cold(&store, tip).await);
    assert_eq!(
        incremental.entries().collect::<Vec<_>>(),
        [("a", "b"), ("b", "a"), ("x", "y/x")]
    );
    let walk = store.first_parent_renames(tip, Some(first)).await.expect("walk");
    assert!(walk.reached_since);
    assert_eq!(walk.commits, 3, "only the commits since the last walk");
}

#[tokio::test]
async fn a_rewritten_history_is_walked_in_full() {
    let repo = Repo::new();
    let seed = commit_files(
        &repo.upstream,
        BRANCH,
        &[("README.md", "# home\n"), ("a.md", "# A\n")],
        "seed",
    );
    let gone = repo.mv("a.md", "b.md", b"# A\n");
    let store = repo.store("r").await;
    let before = cold(&store, gone).await;
    assert_eq!(before.entries().count(), 1);

    // Force-push: the branch now holds a different history from the seed.
    let git = git2::Repository::open_bare(&repo.upstream).expect("upstream");
    git.reference(&format!("refs/heads/{BRANCH}"), seed, true, "rewrite")
        .expect("reset");
    let tip = repo.mv("a.md", "c.md", b"# A\n");
    let guard = store.lock().await;
    store.fetch(&guard).await.expect("fetch");
    drop(guard);

    let rebuilt = Redirects::build(&store, Some(&before), tip).await.expect("rebuild");
    assert_eq!(rebuilt, cold(&store, tip).await);
    assert_eq!(
        rebuilt.entries().collect::<Vec<_>>(),
        [("a", "c")],
        "the lost move is gone"
    );
}

#[tokio::test]
async fn the_first_snapshot_after_open_already_holds_the_redirects() {
    let repo = Repo::new();
    commit_files(
        &repo.upstream,
        BRANCH,
        &[("README.md", "# home\n"), ("a.md", "# A\n")],
        "seed",
    );
    let tip = repo.mv("a.md", "b.md", b"# A\n");
    let wiki = Wiki::open(&repo.config("w")).await.expect("open");
    wiki.poll().await.expect("poll");
    // A restart: `open` publishes refs/riki/good, its one full walk awaited before it returns.
    drop(wiki);
    let wiki = Wiki::open(&repo.config("w")).await.expect("reopen");
    let good = wiki.good().expect("published by open");
    assert_eq!(good.commit(), tip);
    assert_eq!(good.redirects.entries().collect::<Vec<_>>(), [("a", "b")]);
}

#[tokio::test]
async fn publish_extends_the_map_with_each_new_tip() {
    let repo = Repo::new();
    commit_files(
        &repo.upstream,
        BRANCH,
        &[("README.md", "# home\n"), ("a.md", "# A\n")],
        "seed",
    );
    let wiki = Wiki::open(&repo.config("w")).await.expect("open");
    wiki.poll().await.expect("poll");
    assert_eq!(wiki.good().expect("good").redirects.entries().count(), 0);
    let tip = repo.mv("a.md", "b.md", b"# A\n");
    wiki.poll().await.expect("poll");
    let good = wiki.good().expect("good");
    assert_eq!(good.commit(), tip);
    assert_eq!(good.redirects.walked(), Some(tip));
    assert_eq!(good.redirects.entries().collect::<Vec<_>>(), [("a", "b")]);
}
