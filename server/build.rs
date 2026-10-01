//! Build script: bakes git facts into the binary at compile time.

use std::process::Command;

/// Run a `git` command and return its trimmed stdout, or `"unknown"` when git
/// is absent, fails to spawn, exits non-zero, or prints nothing.
///
/// `Command::output()` returns `Ok` even on a non-zero exit, so checking only
/// the spawn result (`.unwrap_or_else`) would silently bake an empty string
/// into the build when run outside a git checkout (e.g. from a source tarball).
fn git_value(args: &[&str]) -> String {
    match Command::new("git").args(args).output() {
        Ok(output) if output.status.success() => {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if value.is_empty() { "unknown".to_string() } else { value }
        }
        _ => "unknown".to_string(),
    }
}

fn main() {
    let git_describe = git_value(&["describe", "--tags", "--always", "--dirty"]);
    let git_sha = git_value(&["rev-parse", "--short", "HEAD"]);
    let git_branch = git_value(&["rev-parse", "--abbrev-ref", "HEAD"]);
    let git_revision = git_value(&["rev-parse", "HEAD"]);

    println!("cargo:rustc-env=GIT_DESCRIBE={git_describe}");
    println!("cargo:rustc-env=GIT_SHA={git_sha}");
    println!("cargo:rustc-env=GIT_BRANCH={git_branch}");
    println!("cargo:rustc-env=GIT_REVISION={git_revision}");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/");
}
