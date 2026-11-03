mod common;

use common::project::Project;

use indoc::indoc;
use predicates::str::contains;

const MANIFEST: &str = indoc! {r#"
    [package]
    name = "acme/my-project"
    type = "project"

    [require]
    "vendor/alpha" = "^1.0"
"#};

const LOCK: &str = indoc! {r#"
    version      = 1
    content-hash = "sha256:test"

    [metadata]
    generated-by = "phast 0.1.0"
    generated-at = "2026-01-01T00:00:00+00:00"

    [[packages]]
    name    = "vendor/alpha"
    version = "1.0.0"
    require = { "vendor/beta" = "^2.0" }

    [[packages]]
    name    = "vendor/beta"
    version = "2.0.0"
"#};

/// A project whose lock matches its manifest.
fn project() -> Project {
    Project::new().manifest(MANIFEST).lock(LOCK)
}

// ---------------------------------------------------------------------------
// Missing files
// ---------------------------------------------------------------------------

#[test]
fn why_fails_without_lock() {
    let project = Project::new().manifest(MANIFEST);

    project
        .cmd()
        .args(["why", "vendor/alpha"])
        .assert()
        .failure();
}

#[test]
fn why_fails_without_manifest() {
    let project = Project::new().lock(LOCK);

    project
        .cmd()
        .args(["why", "vendor/alpha"])
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// Unknown package
// ---------------------------------------------------------------------------

#[test]
fn why_fails_for_package_not_in_lock() {
    let project = project();

    project
        .cmd()
        .args(["why", "vendor/nonexistent"])
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// Successful output
// ---------------------------------------------------------------------------

#[test]
fn why_shows_root_as_direct_requirer() {
    let project = project();

    // vendor/alpha is in root [require], so root should appear as requirer
    project
        .cmd()
        .args(["why", "vendor/alpha"])
        .assert()
        .success()
        .stdout(contains("root"));
}

#[test]
fn why_shows_locked_package_as_requirer() {
    let project = project();

    // vendor/beta is required by vendor/alpha (in lock)
    project
        .cmd()
        .args(["why", "vendor/beta"])
        .assert()
        .success()
        .stdout(contains("vendor/alpha"));
}

// ---------------------------------------------------------------------------
// Flag acceptance
// ---------------------------------------------------------------------------

#[test]
fn why_accepts_tree_flag() {
    let project = project();

    let output = project
        .cmd()
        .args(["why", "vendor/alpha", "--tree"])
        .assert()
        .success()
        .get_output()
        .clone();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unexpected argument"),
        "flag was rejected: {stderr}"
    );
}

#[test]
fn why_accepts_no_dev_flag() {
    let project = project();

    let output = project
        .cmd()
        .args(["why", "vendor/alpha", "--no-dev"])
        .assert()
        .success()
        .get_output()
        .clone();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unexpected argument"),
        "flag was rejected: {stderr}"
    );
}

#[test]
fn why_accepts_project_dir_flag() {
    let project = Project::new().manifest(MANIFEST);

    // No lock: should fail on missing lock, not on unrecognized flag.
    let output = project
        .cmd()
        .args([
            "why",
            "vendor/alpha",
            "--project-dir",
            project.root().to_str().unwrap(),
        ])
        .assert()
        .failure()
        .get_output()
        .clone();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unexpected argument"),
        "flag was rejected: {stderr}"
    );
}
