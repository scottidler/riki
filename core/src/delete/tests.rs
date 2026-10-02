use std::path::PathBuf;
use std::time::Duration;

use tempfile::TempDir;

use super::*;
use crate::store::StoreConfig;
use crate::testing::{commit, commit_files, file_at, file_url, head, history_len, init_upstream};

const BRANCH: &str = "main";

struct Fixture {
    tmp: TempDir,
    upstream: PathBuf,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let tmp = TempDir::new().expect("tmp");
        let upstream = tmp.path().join("upstream.git");
        init_upstream(&upstream);
        commit_files(&upstream, BRANCH, files, "seed");
        Self { tmp, upstream }
    }

    async fn wiki(&self) -> Wiki {
        let wiki = Wiki::open(&StoreConfig {
            remote: file_url(&self.upstream),
            branch: BRANCH.to_string(),
            cache_dir: self.tmp.path().join("content.git"),
            timeout: Duration::from_secs(30),
        })
        .await
        .expect("open");
        wiki.poll().await.expect("poll");
        wiki
    }

    fn message(&self, commit: Oid) -> String {
        let repo = git2::Repository::open_bare(&self.upstream).expect("repo");
        let commit = repo.find_commit(commit).expect("commit");
        commit.message().expect("utf-8").to_string()
    }
}

fn settings() -> SaveSettings {
    SaveSettings {
        committer: Signer {
            name: "riki".to_string(),
            email: "riki@localhost".to_string(),
        },
        push_retries: 1,
    }
}

fn alice() -> Signer {
    Signer {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
    }
}

async fn oid_of(wiki: &Wiki, path: &str) -> Oid {
    let tip = wiki.store().tip().await.expect("tip").expect("a tip");
    wiki.store().entry_at(tip, path).await.expect("read").expect("present")
}

fn delete_request(path: &str, base_oid: Oid) -> DeleteRequest {
    DeleteRequest {
        path: path.to_string(),
        base_oid,
        message: None,
    }
}

fn restore_request(path: &str, commit: Oid) -> RestoreRequest {
    RestoreRequest {
        path: path.to_string(),
        commit,
    }
}

async fn delete_page(wiki: &Wiki, path: &str) -> Oid {
    let base = oid_of(wiki, path).await;
    match delete(wiki, &settings(), &alice(), &delete_request(path, base))
        .await
        .expect("delete")
    {
        WriteOutcome::Pushed { commit } => commit,
        outcome => panic!("expected Pushed, got {outcome:?}"),
    }
}

async fn restore_page(wiki: &Wiki, path: &str, commit: Oid) -> WriteOutcome<OpAnswer> {
    restore(wiki, &settings(), &alice(), &restore_request(path, commit))
        .await
        .expect("restore")
}

#[tokio::test]
async fn delete_then_restore_is_two_commits_and_the_original_bytes() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("a/x.md", "# X\n\nbody\n")]);
    let wiki = fx.wiki().await;
    let d = delete_page(&wiki, "a/x.md").await;
    assert_eq!(head(&fx.upstream, BRANCH), Some(d));
    assert_eq!(file_at(&fx.upstream, BRANCH, "a/x.md"), None);
    assert_eq!(fx.message(d), "riki: delete a/x.md");
    assert_eq!(wiki.good().expect("good").commit(), d, "published");

    let outcome = restore_page(&wiki, "a/x.md", d).await;
    let WriteOutcome::Pushed { commit } = outcome else {
        panic!("expected Pushed, got {outcome:?}");
    };
    assert_eq!(history_len(&fx.upstream, BRANCH), 3, "seed + delete + restore");
    assert_eq!(file_at(&fx.upstream, BRANCH, "a/x.md"), Some(b"# X\n\nbody\n".to_vec()));
    assert_eq!(
        fx.message(commit),
        format!("Revert \"riki: delete a/x.md\"\n\nThis reverts commit {d}.")
    );
    assert_eq!(wiki.good().expect("good").commit(), commit, "published");
}

#[tokio::test]
async fn delete_with_a_stale_base_oid_is_a_conflict_and_nothing_pushed() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("x.md", "# X\n")]);
    let wiki = fx.wiki().await;
    let base = oid_of(&wiki, "x.md").await;
    let laptop = commit_files(&fx.upstream, BRANCH, &[("x.md", "# X edited\n")], "laptop");
    let outcome = delete(&wiki, &settings(), &alice(), &delete_request("x.md", base))
        .await
        .expect("delete");
    assert!(
        matches!(outcome, WriteOutcome::Op(OpAnswer::Conflict(_))),
        "{outcome:?}"
    );
    assert_eq!(head(&fx.upstream, BRANCH), Some(laptop), "nothing pushed");
    assert_eq!(
        wiki.good().expect("good").commit(),
        laptop,
        "the conflict publishes the tip"
    );
}

#[tokio::test]
async fn delete_of_a_path_that_became_a_directory_is_a_conflict() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("x.md", "# X\n")]);
    let wiki = fx.wiki().await;
    let base = oid_of(&wiki, "x.md").await;
    commit(&fx.upstream, BRANCH, &[("x.md", None)], "rm file");
    let laptop = commit_files(&fx.upstream, BRANCH, &[("x.md/inner.md", "# inner\n")], "dir");
    let outcome = delete(&wiki, &settings(), &alice(), &delete_request("x.md", base))
        .await
        .expect("delete");
    assert!(
        matches!(outcome, WriteOutcome::Op(OpAnswer::Conflict(_))),
        "{outcome:?}"
    );
    assert_eq!(head(&fx.upstream, BRANCH), Some(laptop));
}

#[tokio::test]
async fn delete_of_an_absent_page_is_content_present_with_no_commit() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("x.md", "# X\n")]);
    let wiki = fx.wiki().await;
    let base = oid_of(&wiki, "x.md").await;
    let laptop = commit(&fx.upstream, BRANCH, &[("x.md", None)], "laptop rm");
    let outcome = delete(&wiki, &settings(), &alice(), &delete_request("x.md", base))
        .await
        .expect("delete");
    assert_eq!(outcome, WriteOutcome::Op(OpAnswer::ContentPresent));
    assert_eq!(head(&fx.upstream, BRANCH), Some(laptop), "no commit");
    assert_eq!(
        wiki.good().expect("good").commit(),
        laptop,
        "the tip was published first"
    );
}

#[tokio::test]
async fn delete_refuses_the_root_readme_bad_paths_and_multiline_messages() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("a/README.md", "# a\n")]);
    let wiki = fx.wiki().await;
    let readme = oid_of(&wiki, "README.md").await;
    for path in ["README.md", "x.txt", "../x.md", "status.md"] {
        let outcome = delete(&wiki, &settings(), &alice(), &delete_request(path, readme))
            .await
            .expect("delete");
        assert!(
            matches!(outcome, WriteOutcome::Op(OpAnswer::BadRequest(_))),
            "{path}: {outcome:?}"
        );
    }
    let mut two_lines = delete_request("a/README.md", oid_of(&wiki, "a/README.md").await);
    two_lines.message = Some("one\ntwo".to_string());
    let outcome = delete(&wiki, &settings(), &alice(), &two_lines).await.expect("delete");
    assert!(
        matches!(outcome, WriteOutcome::Op(OpAnswer::BadRequest(_))),
        "{outcome:?}"
    );
    assert_eq!(history_len(&fx.upstream, BRANCH), 1);

    two_lines.message = Some("drop folder index".to_string());
    let outcome = delete(&wiki, &settings(), &alice(), &two_lines).await.expect("delete");
    let WriteOutcome::Pushed { commit } = outcome else {
        panic!("a folder README is deletable: {outcome:?}");
    };
    assert_eq!(
        fx.message(commit),
        "drop folder index",
        "the user message replaces the first line"
    );
}

#[tokio::test]
async fn restore_after_recreation_with_different_bytes_is_a_conflict() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("x.md", "# X\n")]);
    let wiki = fx.wiki().await;
    let d = delete_page(&wiki, "x.md").await;
    let laptop = commit_files(&fx.upstream, BRANCH, &[("x.md", "# other\n")], "recreate");
    let outcome = restore_page(&wiki, "x.md", d).await;
    assert!(
        matches!(outcome, WriteOutcome::Op(OpAnswer::Conflict(_))),
        "{outcome:?}"
    );
    assert_eq!(head(&fx.upstream, BRANCH), Some(laptop), "nothing pushed");
}

#[tokio::test]
async fn restore_after_recreation_with_the_same_bytes_is_content_present_with_no_commit() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("x.md", "# X\n")]);
    let wiki = fx.wiki().await;
    let d = delete_page(&wiki, "x.md").await;
    let laptop = commit_files(&fx.upstream, BRANCH, &[("x.md", "# X\n")], "recreate");
    let outcome = restore_page(&wiki, "x.md", d).await;
    assert_eq!(outcome, WriteOutcome::Op(OpAnswer::ContentPresent));
    assert_eq!(head(&fx.upstream, BRANCH), Some(laptop), "no commit");
    assert_eq!(
        wiki.good().expect("good").commit(),
        laptop,
        "the tip was published first"
    );
}

#[tokio::test]
async fn restore_where_a_directory_now_sits_is_a_conflict() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("x.md", "# X\n")]);
    let wiki = fx.wiki().await;
    let d = delete_page(&wiki, "x.md").await;
    commit_files(&fx.upstream, BRANCH, &[("x.md/inner.md", "# inner\n")], "dir");
    let outcome = restore_page(&wiki, "x.md", d).await;
    assert!(
        matches!(outcome, WriteOutcome::Op(OpAnswer::Conflict(_))),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn restore_under_a_file_that_took_a_folder_name_is_a_conflict() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("a/x.md", "# X\n")]);
    let wiki = fx.wiki().await;
    let d = delete_page(&wiki, "a/x.md").await;
    let laptop = commit_files(&fx.upstream, BRANCH, &[("a", "not a folder\n")], "file a");
    let outcome = restore_page(&wiki, "a/x.md", d).await;
    let WriteOutcome::Op(OpAnswer::Conflict(reason)) = &outcome else {
        panic!("expected a conflict, got {outcome:?}");
    };
    assert!(reason.contains("a is a file"), "{reason}");
    assert_eq!(head(&fx.upstream, BRANCH), Some(laptop), "nothing pushed");
}

#[tokio::test]
async fn restore_of_an_older_delete_behind_later_commits_works() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("x.md", "# X\n")]);
    let wiki = fx.wiki().await;
    let d = delete_page(&wiki, "x.md").await;
    commit_files(&fx.upstream, BRANCH, &[("later.md", "# later\n")], "laptop");
    let outcome = restore_page(&wiki, "x.md", d).await;
    assert!(matches!(outcome, WriteOutcome::Pushed { .. }), "{outcome:?}");
    assert_eq!(file_at(&fx.upstream, BRANCH, "x.md"), Some(b"# X\n".to_vec()));
    assert_eq!(file_at(&fx.upstream, BRANCH, "later.md"), Some(b"# later\n".to_vec()));
}

#[tokio::test]
async fn restore_refuses_a_commit_that_is_not_a_delete_of_the_path() {
    let fx = Fixture::new(&[("README.md", "# home\n"), ("x.md", "# X\n"), ("y.md", "# Y\n")]);
    let wiki = fx.wiki().await;
    let seed = head(&fx.upstream, BRANCH).expect("seed");
    let d = delete_page(&wiki, "x.md").await;
    let edit = commit_files(&fx.upstream, BRANCH, &[("y.md", "# Y2\n")], "edit y");
    let merge = merge_commit(&fx, &[edit, d]);
    let blob = oid_of(&wiki, "README.md").await;
    let missing = Oid::from_str("0000000000000000000000000000000000000001").expect("oid");
    // A commit built locally on the tip but never pushed: not in the branch history.
    let tip = wiki.store().tip().await.expect("tip").expect("tip");
    let unpushed = wiki
        .store()
        .commit_tree(
            &[TreeOp::Remove { path: "y.md".into() }],
            tip,
            &alice(),
            &alice(),
            "never pushed",
        )
        .await
        .expect("commit");
    let before = history_len(&fx.upstream, BRANCH);
    for (path, commit, why) in [
        ("x.md", missing, "not a commit"),
        ("x.md", blob, "not a commit"),
        ("y.md", unpushed, "not in the branch history"),
        ("y.md", edit, "did not remove"),
        ("x.md", edit, "is not a file before"),
        ("y.md", d, "did not remove"),
        ("x.md", merge, "parents"),
        ("x.md", seed, "parents"),
    ] {
        let outcome = restore_page(&wiki, path, commit).await;
        let WriteOutcome::Op(OpAnswer::BadRequest(message)) = &outcome else {
            panic!("{path} {commit}: expected BadRequest, got {outcome:?}");
        };
        assert!(message.contains(why), "{path} {commit}: {message}");
    }
    assert_eq!(history_len(&fx.upstream, BRANCH), before, "nothing pushed");
    let outcome = restore_page(&wiki, "../x.md", d).await;
    assert!(
        matches!(outcome, WriteOutcome::Op(OpAnswer::BadRequest(_))),
        "{outcome:?}"
    );
}

/// A merge commit of `parents` on the branch (tree of the first parent), so D recognition sees
/// two parents.
fn merge_commit(fx: &Fixture, parents: &[Oid]) -> Oid {
    let repo = git2::Repository::open_bare(&fx.upstream).expect("repo");
    let parents: Vec<git2::Commit<'_>> = parents
        .iter()
        .map(|oid| repo.find_commit(*oid).expect("parent"))
        .collect();
    let tree = parents[0].tree().expect("tree");
    let sig = git2::Signature::now("T", "t@example.com").expect("sig");
    let refs: Vec<&git2::Commit<'_>> = parents.iter().collect();
    repo.commit(Some(&format!("refs/heads/{BRANCH}")), &sig, &sig, "merge", &tree, &refs)
        .expect("merge")
}
