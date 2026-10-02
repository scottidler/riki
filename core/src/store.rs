//! The git store: riki's bare clone of the content repo.
//!
//! Objects go through git2 (every call inside `spawn_blocking`); the network goes through the `git`
//! CLI, so it reuses whatever credential the host already has. One `tokio::sync::Mutex` per repo
//! serializes fetch, push, and ref updates: two concurrent `git fetch` runs on one repo collide on
//! ref lock files. Reads take no lock.

use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::Duration;

use git2::build::TreeUpdateBuilder;
use git2::{ErrorCode, FileMode, ObjectType, Oid, Repository, Signature, TreeWalkMode, TreeWalkResult};
use thiserror::Error;
use tokio::process::Command;
use tokio::sync::{Mutex, MutexGuard};
use tracing::{debug, info};

use crate::path::{self, PathError};

/// The local ref that holds the good tip: the newest tip whose nav index built without errors.
pub const GOOD_REF: &str = "refs/riki/good";

const REMOTE: &str = "origin";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("git: {0}")]
    Git(#[from] git2::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("blocking task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("{command} timed out after {timeout:?}")]
    Timeout { command: String, timeout: Duration },
    #[error("{command} failed ({}): {stderr}", code.map_or_else(|| "killed by signal".to_string(), |c| format!("exit {c}")))]
    Failed {
        command: String,
        /// `None` when the process was killed by a signal.
        code: Option<i32>,
        stderr: String,
    },
}

/// What the store needs from config.
#[derive(Debug, Clone)]
pub struct StoreConfig {
    pub remote: String,
    pub branch: String,
    pub cache_dir: PathBuf,
    pub timeout: Duration,
}

/// A git author or committer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signer {
    pub name: String,
    pub email: String,
}

/// One file change on top of a parent commit, as a new commit object (no ref moves).
#[derive(Debug, Clone)]
pub struct FileCommit<'a> {
    pub parent: Oid,
    pub path: &'a str,
    pub contents: &'a [u8],
    pub author: &'a Signer,
    pub committer: &'a Signer,
    pub message: &'a str,
}

/// What a push that git completed did. A timeout is `StoreError::Timeout`; any other failure
/// (transport, auth, a remote-side rejection such as a protected branch) is `StoreError::Failed`
/// carrying git's stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushOutcome {
    Pushed,
    /// The remote branch moved: porcelain `!` with `(fetch first)` or `(non-fast-forward)`, or
    /// `(incorrect old value provided)` when a concurrent push moved it between the remote's ref
    /// advertisement and its ref update.
    NonFastForward {
        line: String,
    },
}

/// Proof that the caller holds the repo mutex. Network and ref-writing calls require one, so a
/// caller can hold the lock across several steps (fetch, check, publish).
pub struct RepoGuard<'a> {
    _held: MutexGuard<'a, ()>,
}

#[derive(Debug)]
pub struct GitStore {
    dir: PathBuf,
    branch: String,
    timeout: Duration,
    lock: Mutex<()>,
}

impl GitStore {
    /// Open the bare clone at `cache_dir`, creating it when absent, and point `origin` at the
    /// configured remote with the one refspec riki fetches (a bare clone writes none). Does not
    /// touch the network.
    pub async fn open(config: &StoreConfig) -> Result<Self, StoreError> {
        let dir = config.cache_dir.clone();
        let remote = config.remote.clone();
        let refspec = refspec(&config.branch);
        debug!(
            "GitStore::open: dir={} remote={remote} refspec={refspec}",
            dir.display()
        );
        let repo_dir = dir.clone();
        tokio::task::spawn_blocking(move || configure(&repo_dir, &remote, &refspec)).await??;
        Ok(Self {
            dir,
            branch: config.branch.clone(),
            timeout: config.timeout,
            lock: Mutex::new(()),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Take the repo mutex.
    pub async fn lock(&self) -> RepoGuard<'_> {
        RepoGuard {
            _held: self.lock.lock().await,
        }
    }

    /// `git fetch origin` under the held mutex, bounded by `git.timeout`.
    pub async fn fetch(&self, guard: &RepoGuard<'_>) -> Result<(), StoreError> {
        let _ = guard;
        let mut command = Command::new("git");
        command
            .arg("--git-dir")
            .arg(&self.dir)
            .args(["fetch", "--quiet", REMOTE]);
        run(command, "git fetch", self.timeout).await.map(|_| ())
    }

    /// `git push --porcelain origin <commit>:refs/heads/<branch>` under the held mutex, bounded by
    /// `git.timeout`.
    pub async fn push(&self, guard: &RepoGuard<'_>, commit: Oid) -> Result<PushOutcome, StoreError> {
        let _ = guard;
        let mut command = Command::new("git");
        command
            .arg("--git-dir")
            .arg(&self.dir)
            .args(["push", "--porcelain", REMOTE])
            .arg(format!("{commit}:refs/heads/{}", self.branch));
        let output = run_output(command, "git push", self.timeout).await?;
        let outcome = classify_push(&output);
        info!("GitStore::push: {commit} -> {outcome:?}");
        outcome
    }

    /// Point `refs/remotes/origin/<branch>` at `commit` (after a push landed it), under the held
    /// mutex.
    pub async fn set_tip(&self, guard: &RepoGuard<'_>, commit: Oid) -> Result<(), StoreError> {
        let _ = guard;
        let name = tracking_ref(&self.branch);
        info!("GitStore::set_tip: {name} -> {commit}");
        self.blocking(move |repo| {
            repo.reference(&name, commit, true, "riki: push")?;
            Ok(())
        })
        .await
    }

    /// Write `change` as a blob, a tree, and a commit object. Moves no ref, so it needs no lock.
    pub async fn commit_file(&self, change: FileCommit<'_>) -> Result<Oid, StoreError> {
        path::validate(change.path)?;
        let parent = change.parent;
        let path = change.path.to_string();
        let contents = change.contents.to_vec();
        let author = change.author.clone();
        let committer = change.committer.clone();
        let message = change.message.to_string();
        self.blocking(move |repo| {
            let parent = repo.find_commit(parent)?;
            let blob = repo.blob(&contents)?;
            let mut update = TreeUpdateBuilder::new();
            update.upsert(path.as_str(), blob, FileMode::Blob);
            let tree = repo.find_tree(update.create_updated(repo, &parent.tree()?)?)?;
            let author = Signature::now(&author.name, &author.email)?;
            let committer = Signature::now(&committer.name, &committer.email)?;
            let commit = repo.commit(None, &author, &committer, &message, &tree, &[&parent])?;
            debug!("commit_file: {path} blob={blob} commit={commit}");
            Ok(commit)
        })
        .await
    }

    /// The tip: the commit at `refs/remotes/origin/<branch>`, or `None` before the first fetch.
    pub async fn tip(&self) -> Result<Option<Oid>, StoreError> {
        let name = tracking_ref(&self.branch);
        self.blocking(move |repo| lookup_ref(repo, &name)).await
    }

    /// The good tip recorded in `refs/riki/good`, or `None` before the first publish.
    pub async fn good(&self) -> Result<Option<Oid>, StoreError> {
        self.blocking(|repo| lookup_ref(repo, GOOD_REF)).await
    }

    /// Point `refs/riki/good` at `commit`, under the held mutex.
    pub async fn set_good(&self, guard: &RepoGuard<'_>, commit: Oid) -> Result<(), StoreError> {
        let _ = guard;
        info!("GitStore::set_good: {GOOD_REF} -> {commit}");
        self.blocking(move |repo| {
            repo.reference(GOOD_REF, commit, true, "riki: publish")?;
            Ok(())
        })
        .await
    }

    /// Every blob path in `commit`'s tree, as raw `/`-separated bytes.
    pub async fn blob_paths(&self, commit: Oid) -> Result<Vec<Vec<u8>>, StoreError> {
        self.blocking(move |repo| {
            let tree = repo.find_commit(commit)?.tree()?;
            let mut paths = Vec::new();
            tree.walk(TreeWalkMode::PreOrder, |root, entry| {
                if entry.kind() == Some(ObjectType::Blob) {
                    let mut path = root.as_bytes().to_vec();
                    path.extend_from_slice(entry.name_bytes());
                    paths.push(path);
                }
                TreeWalkResult::Ok
            })?;
            Ok(paths)
        })
        .await
    }

    /// The blob at `path` in `commit`, or `None` when the path is absent or not a file. The path
    /// is validated first; a bad path is a typed error.
    pub async fn read_blob(&self, commit: Oid, path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.blob_at(commit, path).await?.map(|(_, bytes)| bytes))
    }

    /// The blob at `path` in `commit` with its oid, or `None` when the path is absent or not a
    /// file. The path is validated first; a bad path is a typed error.
    pub async fn blob_at(&self, commit: Oid, path: &str) -> Result<Option<(Oid, Vec<u8>)>, StoreError> {
        path::validate(path)?;
        let path = path.to_string();
        self.blocking(move |repo| {
            let tree = repo.find_commit(commit)?.tree()?;
            let entry = match tree.get_path(Path::new(&path)) {
                Ok(entry) => entry,
                Err(err) if err.code() == ErrorCode::NotFound => return Ok(None),
                Err(err) => return Err(err.into()),
            };
            if entry.kind() != Some(ObjectType::Blob) {
                return Ok(None);
            }
            Ok(Some((entry.id(), repo.find_blob(entry.id())?.content().to_vec())))
        })
        .await
    }

    /// The blob with id `oid`, or `None` when the object DB has no such blob.
    pub async fn blob(&self, oid: Oid) -> Result<Option<Vec<u8>>, StoreError> {
        self.blocking(move |repo| match repo.find_blob(oid) {
            Ok(blob) => Ok(Some(blob.content().to_vec())),
            Err(err) if err.code() == ErrorCode::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        })
        .await
    }

    async fn blocking<T, F>(&self, work: F) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Repository) -> Result<T, StoreError> + Send + 'static,
    {
        let dir = self.dir.clone();
        tokio::task::spawn_blocking(move || work(&Repository::open_bare(&dir)?)).await?
    }
}

fn refspec(branch: &str) -> String {
    format!("+refs/heads/{branch}:{}", tracking_ref(branch))
}

fn tracking_ref(branch: &str) -> String {
    format!("refs/remotes/{REMOTE}/{branch}")
}

fn lookup_ref(repo: &Repository, name: &str) -> Result<Option<Oid>, StoreError> {
    match repo.refname_to_id(name) {
        Ok(oid) => Ok(Some(oid)),
        Err(err) if err.code() == ErrorCode::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// Open or init the bare repo, then make `origin` exactly the configured URL and refspec. Re-run on
/// every start, so changing `content.remote` or `content.branch` in config takes effect.
fn configure(dir: &Path, remote: &str, refspec: &str) -> Result<(), StoreError> {
    let repo = if dir.exists() {
        Repository::open_bare(dir)?
    } else {
        info!("configure: creating bare repo at {}", dir.display());
        std::fs::create_dir_all(dir)?;
        Repository::init_bare(dir)?
    };
    let mut config = repo.config()?;
    config.set_str(&format!("remote.{REMOTE}.url"), remote)?;
    let fetch_key = format!("remote.{REMOTE}.fetch");
    match config.remove_multivar(&fetch_key, ".*") {
        Ok(()) => {}
        Err(err) if err.code() == ErrorCode::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    repo.remote_add_fetch(REMOTE, refspec)?;
    Ok(())
}

/// Run a command with a timeout: `kill_on_drop` so a timed-out child dies with its future, stdout
/// and stderr drained concurrently by `wait_with_output`, and the prompt for credentials disabled
/// so a missing credential fails instead of hanging until the timeout. Returns stdout.
pub(crate) async fn run(command: Command, label: &str, timeout: Duration) -> Result<Vec<u8>, StoreError> {
    let output = run_output(command, label, timeout).await?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(failed(label, &output, ""))
    }
}

/// [`run`], but a non-zero exit is returned as the output rather than an error, for callers that
/// read stdout on failure (`git push --porcelain`).
async fn run_output(mut command: Command, label: &str, timeout: Duration) -> Result<Output, StoreError> {
    debug!("run_output: {label} timeout={timeout:?}");
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command.spawn()?;
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(output) => Ok(output?),
        Err(_) => Err(StoreError::Timeout {
            command: label.to_string(),
            timeout,
        }),
    }
}

fn failed(label: &str, output: &Output, prefix: &str) -> StoreError {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    StoreError::Failed {
        command: label.to_string(),
        code: output.status.code(),
        stderr: if prefix.is_empty() {
            stderr
        } else {
            format!("{prefix}\n{stderr}")
        },
    }
}

/// Porcelain rejection reasons that mean "the remote branch moved": retry from a fresh fetch.
/// The last three are the server-side compare-and-swap losing a concurrent push: stock
/// receive-pack says `incorrect old value provided`; ref-transaction failures (GitHub's reported
/// wording) say `cannot lock ref '<ref>': is at <oid> but expected <oid>` or `failed to update
/// ref`. A bare `[remote rejected]` (hooks, protected branches, atomic-push aborts) is not here:
/// retrying cannot fix it.
/// Each entry matches when every one of its fragments is on the `!` line. `(failed to update
/// ref)` keeps its parenthesis so the atomic-push `(failed to update refs)` does not match.
const MOVED_REASONS: &[&[&str]] = &[
    &["(fetch first)"],
    &["(non-fast-forward)"],
    &["(incorrect old value provided)"],
    &["(cannot lock ref '", "': is at ", " but expected "],
    &["(failed to update ref)"],
];

/// Read a finished `git push --porcelain`. Exit 0 is pushed. A `!` ref line with one of
/// [`MOVED_REASONS`] is a non-fast-forward rejection. Anything else (a `!`
/// line with another reason, or no ref line at all: transport failure, exit 128) is a failure
/// carrying git's stderr, prefixed with the `!` line when there is one.
fn classify_push(output: &Output) -> Result<PushOutcome, StoreError> {
    if output.status.success() {
        return Ok(PushOutcome::Pushed);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let rejected = stdout.lines().find(|line| line.starts_with('!'));
    match rejected {
        Some(line) if is_moved(line) => Ok(PushOutcome::NonFastForward { line: line.to_string() }),
        Some(line) => Err(failed("git push", output, line)),
        None => Err(failed("git push", output, "")),
    }
}

fn is_moved(line: &str) -> bool {
    MOVED_REASONS
        .iter()
        .any(|fragments| fragments.iter().all(|fragment| line.contains(fragment)))
}

#[cfg(test)]
mod tests;
