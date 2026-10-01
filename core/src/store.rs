//! The git store: riki's bare clone of the content repo.
//!
//! Objects go through git2 (every call inside `spawn_blocking`); the network goes through the `git`
//! CLI, so it reuses whatever credential the host already has. One `tokio::sync::Mutex` per repo
//! serializes fetch, push, and ref updates: two concurrent `git fetch` runs on one repo collide on
//! ref lock files. Reads take no lock.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use git2::{ErrorCode, ObjectType, Oid, Repository, TreeWalkMode, TreeWalkResult};
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
            Ok(Some(repo.find_blob(entry.id())?.content().to_vec()))
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
pub(crate) async fn run(mut command: Command, label: &str, timeout: Duration) -> Result<Vec<u8>, StoreError> {
    debug!("run: {label} timeout={timeout:?}");
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command.spawn()?;
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(output) => output?,
        Err(_) => {
            return Err(StoreError::Timeout {
                command: label.to_string(),
                timeout,
            });
        }
    };
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(StoreError::Failed {
            command: label.to_string(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

#[cfg(test)]
mod tests;
