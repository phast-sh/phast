mod common;

use common::project::Project;

use indoc::indoc;

const MANIFEST: &str = indoc! {r#"
    [package]
    name = "acme/my-project"
    type = "project"

    [require]
    "vendor/alpha" = "^1.0"
"#};

const LOCK: &str = indoc! {r#"
    version = 1
    content-hash = "sha256:test"

    [metadata]
    generated-by = "phast 0.1.0"
    generated-at = "2026-01-01T00:00:00+00:00"

    [[packages]]
    name = "vendor/alpha"
    version = "1.0.0"
"#};

/// A project with a manifest but no lock.
fn project() -> Project {
    Project::new().manifest(MANIFEST)
}

#[test]
fn outdated_fails_without_lock() {
    let project = project();

    project.cmd().arg("outdated").assert().failure();
}

#[test]
fn outdated_fails_without_manifest() {
    let project = Project::new().lock(LOCK);

    project.cmd().arg("outdated").assert().failure();
}

#[test]
fn outdated_accepts_project_dir_flag() {
    let project = project();

    // No lock file: should fail, but the --project-dir flag itself must be accepted.
    let output = project
        .cmd()
        .args([
            "outdated",
            "--project-dir",
            project.root().to_str().unwrap(),
        ])
        .assert()
        .failure()
        .get_output()
        .clone();

    // The failure must be about the missing lock, not an unrecognized flag.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unexpected argument"),
        "flag was rejected: {stderr}"
    );
}

#[test]
fn outdated_accepts_no_dev_flag() {
    let project = project();

    let output = project
        .cmd()
        .args(["outdated", "--no-dev"])
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

#[test]
fn outdated_accepts_format_json_flag() {
    let project = project();

    let output = project
        .cmd()
        .args(["outdated", "--format", "json"])
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
