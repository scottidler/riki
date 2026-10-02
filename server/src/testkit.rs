//! Server test fixtures: a local upstream bare repo and a wiki cloned from it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use riki_core::Oid;
use riki_core::store::StoreConfig;
use riki_core::testing::{commit, commit_files, file_url, init_upstream};
use riki_core::wiki::Wiki;
use tempfile::TempDir;

pub const BRANCH: &str = "main";

pub struct Upstream {
    pub tmp: TempDir,
    pub dir: PathBuf,
}

impl Upstream {
    pub fn new() -> Self {
        let tmp = TempDir::new().expect("tmp");
        let dir = tmp.path().join("upstream.git");
        init_upstream(&dir);
        Self { tmp, dir }
    }

    pub fn push(&self, files: &[(&str, &str)]) -> Oid {
        commit_files(&self.dir, BRANCH, files, "push")
    }

    pub fn push_bytes(&self, files: &[(&str, &[u8])]) -> Oid {
        let files: Vec<(&str, Option<&[u8]>)> = files.iter().map(|(p, c)| (*p, Some(*c))).collect();
        commit(&self.dir, BRANCH, &files, "push")
    }

    pub fn store_config(&self) -> StoreConfig {
        self.replica_config("content", Duration::from_secs(30))
    }

    pub async fn wiki(&self) -> Arc<Wiki> {
        Arc::new(Wiki::open(&self.store_config()).await.expect("open wiki"))
    }

    /// The bare clone dir of replica `name`, inside this fixture's tempdir.
    pub fn clone_dir(&self, name: &str) -> PathBuf {
        self.tmp.path().join(format!("{name}.git"))
    }

    /// Store config for replica `name`: its own bare clone of this upstream, `git.timeout` of
    /// `timeout`. Every path stays inside the fixture's tempdir.
    pub fn replica_config(&self, name: &str, timeout: Duration) -> StoreConfig {
        StoreConfig {
            remote: file_url(&self.dir),
            branch: BRANCH.to_string(),
            cache_dir: self.clone_dir(name),
            timeout,
        }
    }

    /// A polled wiki for replica `name`, so it serves the current upstream tip.
    pub async fn replica(&self, name: &str, timeout: Duration) -> Arc<Wiki> {
        let wiki = Wiki::open(&self.replica_config(name, timeout))
            .await
            .expect("open replica");
        wiki.poll().await.expect("poll");
        Arc::new(wiki)
    }
}
