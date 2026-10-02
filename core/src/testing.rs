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

/// The commit `refs/heads/<branch>` points at in the bare repo at `dir`, if any.
pub fn head(dir: &Path, branch: &str) -> Option<Oid> {
    let repo = Repository::open_bare(dir).expect("open repo");
    repo.refname_to_id(&format!("refs/heads/{branch}")).ok()
}

/// How many commits `refs/heads/<branch>` has, following first parents.
pub fn history_len(dir: &Path, branch: &str) -> usize {
    let repo = Repository::open_bare(dir).expect("open repo");
    let mut walk = repo.revwalk().expect("revwalk");
    walk.push_ref(&format!("refs/heads/{branch}")).expect("push ref");
    walk.count()
}

/// The bytes of `path` at `refs/heads/<branch>` in the bare repo at `dir`, if present.
pub fn file_at(dir: &Path, branch: &str, path: &str) -> Option<Vec<u8>> {
    let repo = Repository::open_bare(dir).expect("open repo");
    let commit = repo.find_commit(head(dir, branch)?).expect("head commit");
    let entry = commit.tree().expect("tree").get_path(Path::new(path)).ok()?;
    Some(repo.find_blob(entry.id()).expect("blob").content().to_vec())
}

/// The author email of the commit at `refs/heads/<branch>`.
pub fn head_author_email(dir: &Path, branch: &str) -> String {
    let repo = Repository::open_bare(dir).expect("open repo");
    let commit = repo.find_commit(head(dir, branch).expect("head")).expect("commit");
    commit.author().email().expect("utf-8 email").to_string()
}

/// Make the bare clone at `clone_dir` run `program` instead of `git-receive-pack` on the remote
/// side of a push (`remote.origin.receivepack`). Over a `file://` remote git runs it through the
/// shell with the remote path appended, so a test can stall or fail a push without any network.
pub fn set_receive_pack(clone_dir: &Path, program: &Path) {
    let repo = Repository::open_bare(clone_dir).expect("open clone");
    repo.config()
        .expect("config")
        .set_str("remote.origin.receivepack", &program.display().to_string())
        .expect("set receivepack");
}

/// Write an executable `/bin/sh` script.
pub fn write_script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("write script");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod script");
}
