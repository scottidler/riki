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
async fn read_blobs_returns_the_present_files_in_one_pass() {
    let tmp = TempDir::new().expect("tmp");
    let up = upstream(&tmp);
    let commit = commit_files(
        &up,
        BRANCH,
        &[("README.md", "# home\n"), ("a/b/c.md", "deep\n")],
        "seed",
    );
    let store = GitStore::open(&config(&tmp, file_url(&up))).await.expect("open");
    store.fetch(&store.lock().await).await.expect("fetch");
    let files = ["a/b/c.md", "missing.md", "a/b", "README.md"]
        .map(String::from)
        .to_vec();
    assert_eq!(
        store.read_blobs(commit, files).await.expect("read"),
        [
            ("a/b/c.md".to_string(), b"deep\n".to_vec()),
            ("README.md".to_string(), b"# home\n".to_vec()),
        ],
        "absent paths and trees are left out"
    );
    assert!(matches!(
        store.read_blobs(commit, vec!["../x.md".to_string()]).await,
        Err(StoreError::Path(PathError::DotDot(_)))
    ));
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

fn output(code: i32, stdout: &str, stderr: &str) -> Output {
    use std::os::unix::process::ExitStatusExt;
    Output {
        status: std::process::ExitStatus::from_raw(code << 8),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

#[test]
fn classify_push_reads_phase_0b_outputs() {
    let pushed = output(
        0,
        "To github.com:x/y.git\n \td0b1:refs/heads/main\taf97..d0b1\nDone\n",
        "",
    );
    assert_eq!(classify_push(&pushed).expect("pushed"), PushOutcome::Pushed);
    let fetch_first = output(
        1,
        "To github.com:x/y.git\n!\t599f:refs/heads/main\t[rejected] (fetch first)\nDone\n",
        "error: failed to push some refs",
    );
    assert!(matches!(
        classify_push(&fetch_first),
        Ok(PushOutcome::NonFastForward { line }) if line.contains("(fetch first)")
    ));
    let non_ff = output(1, "!\t599f:refs/heads/main\t[rejected] (non-fast-forward)\nDone\n", "");
    assert!(matches!(classify_push(&non_ff), Ok(PushOutcome::NonFastForward { .. })));
    // Observed with two replicas pushing at once to a local upstream (git 2.53): the loser's
    // receive-pack ref update finds the branch already moved.
    let lost_race = output(
        1,
        "!\te1b0:refs/heads/main\t[remote rejected] (incorrect old value provided)\nDone\n",
        "error: failed to push some refs",
    );
    assert!(matches!(
        classify_push(&lost_race),
        Ok(PushOutcome::NonFastForward { .. })
    ));
}

#[test]
fn classify_push_fails_transport_and_other_rejections_with_stderr() {
    let transport = output(128, "", "fatal: Could not read from remote repository.");
    match classify_push(&transport) {
        Err(StoreError::Failed { code, stderr, .. }) => {
            assert_eq!(code, Some(128));
            assert_eq!(stderr, "fatal: Could not read from remote repository.");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    let protected = output(
        1,
        "!\tabc:refs/heads/main\t[remote rejected] (protected branch hook declined)\nDone\n",
        "remote: error: GH006: Protected branch update failed",
    );
    match classify_push(&protected) {
        Err(StoreError::Failed { stderr, .. }) => {
            assert!(stderr.contains("protected branch hook declined"), "{stderr}");
            assert!(stderr.contains("GH006"), "{stderr}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn classify_push_retries_a_lost_server_side_ref_update() {
    for reason in [
        "(cannot lock ref 'refs/heads/main': is at 1111111111111111111111111111111111111111 but expected 2222222222222222222222222222222222222222)",
        "(failed to update ref)",
    ] {
        let lost = output(
            1,
            &format!("To github.com:x/y.git\n!\te1b0:refs/heads/main\t[remote rejected] {reason}\nDone\n"),
            "error: failed to push some refs",
        );
        assert!(
            matches!(classify_push(&lost), Ok(PushOutcome::NonFastForward { ref line }) if line.contains(reason)),
            "{reason}: {:?}",
            classify_push(&lost)
        );
    }
}

#[test]
fn classify_push_fails_hook_atomic_and_generic_remote_rejections() {
    for reason in [
        "(pre-receive hook declined)",
        "(protected branch hook declined)",
        "(atomic push failure)",
        "(failed to update refs)",
        "(cannot lock ref 'refs/heads/main': reference already exists)",
        "(some new server reason)",
    ] {
        let rejected = output(
            1,
            &format!("!\tabc:refs/heads/main\t[remote rejected] {reason}\nDone\n"),
            "error: failed to push some refs",
        );
        match classify_push(&rejected) {
            Err(StoreError::Failed { stderr, .. }) => assert!(stderr.contains(reason), "{stderr}"),
            other => panic!("{reason}: expected Failed, got {other:?}"),
        }
    }
}

fn signer(name: &str) -> Signer {
    Signer {
        name: name.to_string(),
        email: format!("{name}@example.com"),
    }
}

#[tokio::test]
async fn commit_file_then_push_lands_upstream_and_set_tip_moves_the_tracking_ref() {
    let tmp = TempDir::new().expect("tmp");
    let up = upstream(&tmp);
    let seed = commit_files(&up, BRANCH, &[("README.md", "x\n")], "seed");
    let store = GitStore::open(&config(&tmp, file_url(&up))).await.expect("open");
    let guard = store.lock().await;
    store.fetch(&guard).await.expect("fetch");
    let (author, committer) = (signer("alice"), signer("riki"));
    let commit = store
        .commit_file(FileCommit {
            parent: seed,
            path: "a/b/c.md",
            contents: b"nested\n",
            author: &author,
            committer: &committer,
            message: "riki: edit a/b/c.md",
        })
        .await
        .expect("commit");
    assert_eq!(store.tip().await.expect("tip"), Some(seed), "commit_file moves no ref");
    assert_eq!(store.push(&guard, commit).await.expect("push"), PushOutcome::Pushed);
    assert_eq!(crate::testing::head(&up, BRANCH), Some(commit));
    assert_eq!(crate::testing::head_author_email(&up, BRANCH), "alice@example.com");
    store.set_tip(&guard, commit).await.expect("set tip");
    assert_eq!(store.tip().await.expect("tip"), Some(commit));
    let (oid, bytes) = store.blob_at(commit, "a/b/c.md").await.expect("read").expect("present");
    assert_eq!(bytes, b"nested\n");
    assert_eq!(store.blob(oid).await.expect("blob"), Some(b"nested\n".to_vec()));
    assert_eq!(store.blob(Oid::ZERO_SHA1).await.expect("blob"), None);
}

#[tokio::test]
async fn push_behind_upstream_is_non_fast_forward() {
    let tmp = TempDir::new().expect("tmp");
    let up = upstream(&tmp);
    let seed = commit_files(&up, BRANCH, &[("README.md", "x\n")], "seed");
    let store = GitStore::open(&config(&tmp, file_url(&up))).await.expect("open");
    let guard = store.lock().await;
    store.fetch(&guard).await.expect("fetch");
    commit_files(&up, BRANCH, &[("other.md", "laptop\n")], "laptop");
    let (author, committer) = (signer("alice"), signer("riki"));
    let commit = store
        .commit_file(FileCommit {
            parent: seed,
            path: "mine.md",
            contents: b"mine\n",
            author: &author,
            committer: &committer,
            message: "m",
        })
        .await
        .expect("commit");
    assert!(matches!(
        store.push(&guard, commit).await,
        Ok(PushOutcome::NonFastForward { .. })
    ));
}

#[tokio::test]
async fn commit_file_rejects_bad_paths() {
    let tmp = TempDir::new().expect("tmp");
    let up = upstream(&tmp);
    let seed = commit_files(&up, BRANCH, &[("README.md", "x\n")], "seed");
    let store = GitStore::open(&config(&tmp, file_url(&up))).await.expect("open");
    store.fetch(&store.lock().await).await.expect("fetch");
    let who = signer("alice");
    let result = store
        .commit_file(FileCommit {
            parent: seed,
            path: "a\0.md",
            contents: b"x",
            author: &who,
            committer: &who,
            message: "m",
        })
        .await;
    assert!(matches!(result, Err(StoreError::Path(PathError::Nul(_)))));
}
