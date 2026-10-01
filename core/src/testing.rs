//! Test fixtures: local upstream bare repos built directly with git2, so tests never touch the
//! network. Compiled for this crate's tests and, via the `testing` feature, for the server's.

use std::path::Path;

use git2::build::TreeUpdateBuilder;
use git2::{FileMode, Oid, Repository, Signature};

/// Create an empty bare repo at `dir` to act as the upstream.
pub fn init_upstream(dir: &Path) -> Repository {
    Repository::init_bare(dir).expect("init upstream")
}

/// Commit `files` (path, contents) on top of `refs/heads/<branch>` in the bare repo at `dir`
/// (an orphan commit when the branch does not exist yet) and move the branch to it. Paths not
/// listed carry over from the parent; a `None` contents removes the path.
pub fn commit(dir: &Path, branch: &str, files: &[(&str, Option<&[u8]>)], message: &str) -> Oid {
    let repo = Repository::open_bare(dir).expect("open upstream");
    let refname = format!("refs/heads/{branch}");
    let parent = repo
        .refname_to_id(&refname)
        .ok()
        .map(|oid| repo.find_commit(oid).expect("parent"));
    let baseline = match &parent {
        Some(parent) => parent.tree().expect("parent tree"),
        None => {
            let empty = repo
                .treebuilder(None)
                .expect("treebuilder")
                .write()
                .expect("empty tree");
            repo.find_tree(empty).expect("find empty tree")
        }
    };
    let mut update = TreeUpdateBuilder::new();
    for (path, contents) in files {
        match contents {
            Some(bytes) => {
                let blob = repo.blob(bytes).expect("blob");
                update.upsert(*path, blob, FileMode::Blob);
            }
            None => {
                update.remove(*path);
            }
        }
    }
    let tree_oid = update.create_updated(&repo, &baseline).expect("tree");
    let tree = repo.find_tree(tree_oid).expect("find tree");
    let sig = Signature::now("Test Author", "author@example.com").expect("signature");
    let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
    repo.commit(Some(&refname), &sig, &sig, message, &tree, &parents)
        .expect("commit")
}

/// `commit` with every file present (no removals).
pub fn commit_files(dir: &Path, branch: &str, files: &[(&str, &str)], message: &str) -> Oid {
    let files: Vec<(&str, Option<&[u8]>)> = files.iter().map(|(p, c)| (*p, Some(c.as_bytes()))).collect();
    commit(dir, branch, &files, message)
}

/// A `file://` URL for a local path.
pub fn file_url(dir: &Path) -> String {
    format!("file://{}", dir.display())
}
