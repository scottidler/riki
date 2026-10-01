use std::time::Duration;

use tempfile::TempDir;

use super::*;
use crate::testing::{commit_files, file_url, init_upstream};

const BRANCH: &str = "main";

fn config(tmp: &TempDir, remote: String) -> StoreConfig {
    StoreConfig {
        remote,
        branch: BRANCH.to_string(),
        cache_dir: tmp.path().join("cache").join("content.git"),
        timeout: Duration::from_secs(30),
    }
}

fn upstream(tmp: &TempDir) -> PathBuf {
    let dir = tmp.path().join("upstream.git");
    init_upstream(&dir);
    dir
}

#[tokio::test]
async fn open_creates_a_bare_clone_with_exactly_one_refspec() {
    let tmp = TempDir::new().expect("tmp");
    let up = upstream(&tmp);
    let config = config(&tmp, file_url(&up));
    GitStore::open(&config).await.expect("create");
    GitStore::open(&config).await.expect("reopen");
    let repo = Repository::open_bare(&config.cache_dir).expect("is a bare repo");
    let remote = repo.find_remote("origin").expect("origin");
    assert_eq!(remote.url().expect("url"), file_url(&up));
    let specs: Vec<String> = remote
        .fetch_refspecs()
        .expect("specs")
        .iter()
        .map(|spec| spec.expect("utf-8").expect("present").to_string())
        .collect();
    assert_eq!(specs, ["+refs/heads/main:refs/remotes/origin/main"]);
}

#[tokio::test]
async fn open_rewrites_the_remote_url_from_config() {
    let tmp = TempDir::new().expect("tmp");
    let up = upstream(&tmp);
    GitStore::open(&config(&tmp, "file:///nowhere".to_string()))
        .await
        .expect("first");
    let config = config(&tmp, file_url(&up));
    GitStore::open(&config).await.expect("second");
    let repo = Repository::open_bare(&config.cache_dir).expect("repo");
    assert_eq!(
        repo.find_remote("origin").expect("origin").url().expect("url"),
        file_url(&up)
    );
}

#[tokio::test]
async fn open_fails_loudly_on_a_non_repo_dir() {
    let tmp = TempDir::new().expect("tmp");
    let config = config(&tmp, "file:///x".to_string());
    std::fs::create_dir_all(&config.cache_dir).expect("mkdir");
    assert!(matches!(GitStore::open(&config).await, Err(StoreError::Git(_))));
}

#[tokio::test]
async fn fetch_then_read_blob_by_path() {
    let tmp = TempDir::new().expect("tmp");
    let up = upstream(&tmp);
    let commit = commit_files(
        &up,
        BRANCH,
        &[("README.md", "# home\n"), ("a/b/c.md", "deep\n")],
        "seed",
    );
    let store = GitStore::open(&config(&tmp, file_url(&up))).await.expect("open");
    assert_eq!(store.tip().await.expect("tip"), None);
    store.fetch(&store.lock().await).await.expect("fetch");
    assert_eq!(store.tip().await.expect("tip"), Some(commit));
    assert_eq!(
        store.read_blob(commit, "a/b/c.md").await.expect("read"),
        Some(b"deep\n".to_vec())
    );
    assert_eq!(store.read_blob(commit, "a/missing.md").await.expect("read"), None);
    assert_eq!(
        store.read_blob(commit, "a/b").await.expect("read"),
        None,
        "a tree is not a blob"
    );
    let mut paths = store.blob_paths(commit).await.expect("paths");
    paths.sort();
    assert_eq!(paths, [b"README.md".to_vec(), b"a/b/c.md".to_vec()]);
}

#[tokio::test]
async fn read_blob_rejects_bad_paths_with_typed_errors() {
    let tmp = TempDir::new().expect("tmp");
    let up = upstream(&tmp);
    let commit = commit_files(&up, BRANCH, &[("README.md", "x\n")], "seed");
    let store = GitStore::open(&config(&tmp, file_url(&up))).await.expect("open");
    store.fetch(&store.lock().await).await.expect("fetch");
    assert!(matches!(
        store.read_blob(commit, "../README.md").await,
        Err(StoreError::Path(PathError::DotDot(_)))
    ));
    assert!(matches!(
        store.read_blob(commit, "README\0.md").await,
        Err(StoreError::Path(PathError::Nul(_)))
    ));
    assert!(matches!(
        store.read_blob(commit, ".git/config").await,
        Err(StoreError::Path(PathError::LeadingDot { .. }))
    ));
}

#[tokio::test]
async fn good_ref_round_trips() {
    let tmp = TempDir::new().expect("tmp");
    let up = upstream(&tmp);
    let commit = commit_files(&up, BRANCH, &[("README.md", "x\n")], "seed");
    let store = GitStore::open(&config(&tmp, file_url(&up))).await.expect("open");
    assert_eq!(store.good().await.expect("good"), None);
    let guard = store.lock().await;
    store.fetch(&guard).await.expect("fetch");
    store.set_good(&guard, commit).await.expect("set");
    assert_eq!(store.good().await.expect("good"), Some(commit));
}

#[tokio::test]
async fn fetch_from_a_missing_upstream_is_a_failure_not_a_panic() {
    let tmp = TempDir::new().expect("tmp");
    let store = GitStore::open(&config(&tmp, file_url(&tmp.path().join("gone.git"))))
        .await
        .expect("open");
    let err = store.fetch(&store.lock().await).await.expect_err("unreachable");
    assert!(matches!(err, StoreError::Failed { code: Some(_), .. }), "{err}");
}

#[tokio::test]
async fn run_kills_on_timeout() {
    let mut command = Command::new("sleep");
    command.arg("5");
    let started = std::time::Instant::now();
    let err = run(command, "sleep", Duration::from_millis(100))
        .await
        .expect_err("times out");
    assert!(matches!(err, StoreError::Timeout { .. }), "{err}");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn run_drains_both_streams_and_returns_stdout() {
    let mut command = Command::new("sh");
    // More than a pipe buffer on both streams: a sequential drain would deadlock here.
    command.args(["-c", "head -c 200000 /dev/zero; head -c 200000 /dev/zero >&2"]);
    let stdout = run(command, "sh", Duration::from_secs(10)).await.expect("runs");
    assert_eq!(stdout.len(), 200_000);
}

#[tokio::test]
async fn run_reports_exit_code_and_stderr() {
    let mut command = Command::new("sh");
    command.args(["-c", "echo boom >&2; exit 3"]);
    match run(command, "sh", Duration::from_secs(10)).await {
        Err(StoreError::Failed { code, stderr, .. }) => {
            assert_eq!(code, Some(3));
            assert_eq!(stderr, "boom");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}
