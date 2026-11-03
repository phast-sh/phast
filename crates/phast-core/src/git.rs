//! Minimal git inspection: reading the current branch from `.git/HEAD`.
//!
//! Phast has no local git integration; this is a plain file read used to derive
//! a `dev-<branch>` version for the root and for version-less path packages
//! (a monorepo component checked out on a branch).

use std::path::{Path, PathBuf};

/// The branch name of the git checkout containing `start`, read from `.git/HEAD`.
///
/// Walks up from `start` to the nearest `.git` directory. Returns `None` for a detached HEAD
/// (HEAD holds a bare commit) and when no such directory is found, which includes a linked
/// worktree: there `.git` is a file pointing at the real one, so the walk passes it by.
pub fn current_branch(start: &Path) -> Option<String> {
    let git_dir = find_git_dir(start)?;
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;

    // A branch checkout writes `ref: refs/heads/<branch>`;
    // a detached HEAD writes a bare commit SHA, which has no such prefix.
    head.trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_owned)
}

/// The nearest ancestor `.git` directory of `start`, if any.
fn find_git_dir(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(".git"))
        .find(|candidate| candidate.is_dir())
}
