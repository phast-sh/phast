use phast_core::git::current_branch;

// ---------------------------------------------------------------------------
// Finding the git directory to read
// ---------------------------------------------------------------------------

#[test]
fn walks_up_from_a_nested_directory() {
    let tmp = tempfile::tempdir().unwrap();
    write_head(tmp.path(), "ref: refs/heads/7.4\n");
    let nested = tmp.path().join("src/Symfony/Contracts");
    std::fs::create_dir_all(&nested).unwrap();

    assert_eq!(current_branch(&nested).as_deref(), Some("7.4"));
}

#[test]
fn a_nested_repo_shadows_the_one_above_it() {
    let tmp = tempfile::tempdir().unwrap();
    write_head(tmp.path(), "ref: refs/heads/outer\n");

    let inner = tmp.path().join("vendor/acme/inner");
    std::fs::create_dir_all(&inner).unwrap();
    write_head(&inner, "ref: refs/heads/inner\n");

    // The walk stops at the first `.git` it meets, so a package vendored with its
    // own repository reports its own branch rather than the project's.
    assert_eq!(current_branch(&inner).as_deref(), Some("inner"));
}

#[test]
fn no_git_dir_has_no_branch() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(current_branch(tmp.path()), None);
}

#[test]
fn a_linked_worktree_has_no_branch() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".git"),
        "gitdir: /elsewhere/.git/worktrees/feature\n",
    )
    .unwrap();

    // A known limitation rather than a choice: in a linked worktree `.git` is a
    // file pointing at the real directory, and the walk only looks for a
    // directory, so no `dev-<branch>` version is derived there.
    assert_eq!(current_branch(tmp.path()), None);
}

// ---------------------------------------------------------------------------
// Reading the branch out of HEAD
// ---------------------------------------------------------------------------

#[test]
fn reads_branch_from_head() {
    let tmp = tempfile::tempdir().unwrap();
    write_head(tmp.path(), "ref: refs/heads/main\n");

    assert_eq!(current_branch(tmp.path()).as_deref(), Some("main"));
}

#[test]
fn a_branch_name_can_contain_slashes() {
    let tmp = tempfile::tempdir().unwrap();
    write_head(tmp.path(), "ref: refs/heads/feature/new-resolver\n");

    // Only the `refs/heads/` prefix is removed, so the rest of the ref is the
    // name however many segments it has.
    assert_eq!(
        current_branch(tmp.path()).as_deref(),
        Some("feature/new-resolver"),
    );
}

#[test]
fn detached_head_has_no_branch() {
    let tmp = tempfile::tempdir().unwrap();
    write_head(tmp.path(), "1ef36fa01286573e846ac449e8683f8833c5b26a\n");

    assert_eq!(current_branch(tmp.path()), None);
}

#[test]
fn a_head_pointing_at_a_tag_has_no_branch() {
    let tmp = tempfile::tempdir().unwrap();
    write_head(tmp.path(), "ref: refs/tags/v1.0.0\n");

    // A ref, but not a branch. Checking out a tag is a detached HEAD by another
    // spelling, and it has no branch name to derive a version from either.
    assert_eq!(current_branch(tmp.path()), None);
}

#[test]
fn a_git_dir_without_head_has_no_branch() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();

    assert_eq!(current_branch(tmp.path()), None);
}

/// Write `content` to `dir/.git/HEAD`, creating the `.git` directory.
fn write_head(dir: &std::path::Path, content: &str) {
    let git = dir.join(".git");
    std::fs::create_dir_all(&git).unwrap();
    std::fs::write(git.join("HEAD"), content).unwrap();
}
