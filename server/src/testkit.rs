//! Server test fixtures: a local upstream bare repo and a wiki cloned from it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use riki_core::Oid;
use riki_core::store::StoreConfig;
use riki_core::testing::{commit_files, file_url, init_upstream};
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

    pub fn store_config(&self) -> StoreConfig {
        StoreConfig {
            remote: file_url(&self.dir),
            branch: BRANCH.to_string(),
            cache_dir: self.tmp.path().join("content.git"),
            timeout: Duration::from_secs(30),
        }
    }

    pub async fn wiki(&self) -> Arc<Wiki> {
        Arc::new(Wiki::open(&self.store_config()).await.expect("open wiki"))
    }
}
