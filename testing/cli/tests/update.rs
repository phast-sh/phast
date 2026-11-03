//! The `phast update` command surface.
//!
//! The registry serves two versions of each package so resolution has a choice to
//! make. Without that, every assertion about updating would pass whether or not the
//! command did anything.
//!
//! Naming a package pins every other one to its locked version, so the fixtures start
//! from a lock written at the older versions and assert what moved.

mod common;

use common::project::Project;
use common::registry::MockRegistry;

use indoc::{formatdoc, indoc};
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::json;

const MANIFEST: &str = indoc! {r#"
    [package]
    name = "acme/my-project"
    type = "project"

    [require]
    "vendor/alpha" = "^1.0"
    "vendor/beta" = "^2.0"
"#};

const MANIFEST_WITH_DEV: &str = indoc! {r#"
    [package]
    name = "acme/my-project"
    type = "project"

    [require]
    "vendor/alpha" = "^1.0"

    [require-dev]
    "vendor/beta" = "^2.0"
"#};

/// Lock at the older versions, so an update has somewhere to move to.
///
/// Written by hand rather than by running `lock`, because the registry
/// would resolve straight to the newest and leave nothing to update.
const LOCK: &str = indoc! {r#"
    version      = 1
    content-hash = "sha256:stale"

    [metadata]
    generated-by = "phast 0.1.0"
    generated-at = "2026-01-01T00:00:00+00:00"

    [[packages]]
    name    = "vendor/alpha"
    version = "1.0.0"

    [[packages]]
    name    = "vendor/beta"
    version = "2.0.0"
"#};

fn project() -> Project {
    Project::new().manifest(MANIFEST)
}

/// Both packages, each with a newer version available than `^1.0` / `^2.0` first
/// resolved to.
async fn registry() -> MockRegistry {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/alpha", &["1.0.0", "1.1.0"], json!({}))
        .await;
    registry
        .mount("vendor/beta", &["2.0.0", "2.1.0"], json!({}))
        .await;
    registry
}

/// A workspace where each member requires one of the two packages, so narrowing to a
/// member is observable: the sibling's package has somewhere to move and must not.
fn workspace() -> Project {
    Project::new()
        .manifest(indoc! {r#"
            [workspace]
            members = ["packages/*"]
        "#})
        .write(
            "packages/api/phast.toml",
            indoc! {r#"
                [package]
                name = "acme/api"

                [require]
                "vendor/alpha" = "^1.0"
            "#},
        )
        .write(
            "packages/web/phast.toml",
            indoc! {r#"
                [package]
                name = "acme/web"

                [require]
                "vendor/beta" = "^2.0"
            "#},
        )
        .lock(LOCK)
}

#[tokio::test]
async fn update_moves_every_package_when_none_is_named() {
    let registry = registry().await;
    let project = project().lock(LOCK);

    project
        .cmd()
        .args(["update", "--registry-url", &registry.uri()])
        .assert()
        .success();

    let lock = project.read("phast.lock");
    assert!(lock.contains("1.1.0"), "alpha did not move:\n{lock}");
    assert!(lock.contains("2.1.0"), "beta did not move:\n{lock}");
}

#[tokio::test]
async fn update_pins_the_packages_it_was_not_asked_about() {
    let registry = registry().await;
    let project = project().lock(LOCK);

    project
        .cmd()
        .args(["update", "vendor/alpha", "--registry-url", &registry.uri()])
        .assert()
        .success();

    let lock = project.read("phast.lock");
    assert!(
        lock.contains("1.1.0"),
        "the named package did not move:\n{lock}",
    );
    assert!(
        lock.contains("2.0.0"),
        "an unnamed package moved anyway:\n{lock}",
    );
    assert!(
        !lock.contains("2.1.0"),
        "an unnamed package moved anyway:\n{lock}",
    );
}

#[tokio::test]
async fn update_dry_run_writes_nothing() {
    let registry = registry().await;
    let project = project().lock(LOCK);

    project
        .cmd()
        .args(["update", "--dry-run", "--registry-url", &registry.uri()])
        .assert()
        .success();

    let lock = project.read("phast.lock");
    assert!(
        lock.contains("1.0.0") && !lock.contains("1.1.0"),
        "--dry-run rewrote the lock:\n{lock}"
    );
    assert!(!project.root().join("vendor").exists());
}

#[tokio::test]
async fn update_installs_what_it_resolved() {
    let registry = registry().await;
    let project = project().lock(LOCK);

    project
        .cmd()
        .args(["update", "--registry-url", &registry.uri()])
        .assert()
        .success();

    assert!(project.root().join("vendor/vendor/alpha").is_dir());
    assert!(project.root().join("vendor/autoload.php").exists());
}

#[tokio::test]
async fn update_no_dev_still_locks_the_dev_packages() {
    let registry = registry().await;
    let project = Project::new().manifest(MANIFEST_WITH_DEV);

    project
        .cmd()
        .args(["update", "--no-dev", "--registry-url", &registry.uri()])
        .assert()
        .success();

    assert!(
        project.read("phast.lock").contains("vendor/beta"),
        "--no-dev left the dev package out of the lock",
    );
    assert!(!project.root().join("vendor/vendor/beta").exists());
}

#[tokio::test]
async fn update_runs_the_update_hooks() {
    let manifest = formatdoc! {r#"
        {MANIFEST}
        [scripts]
        pre-update-cmd  = "echo PRE_HOOK"
        post-update-cmd = "echo POST_HOOK"
    "#};
    let registry = registry().await;
    let project = project().manifest(&manifest).lock(LOCK);

    project
        .cmd()
        .args(["update", "--registry-url", &registry.uri()])
        .assert()
        .success()
        .stdout(contains("PRE_HOOK"))
        .stdout(contains("POST_HOOK"));
}

#[tokio::test]
async fn update_no_scripts_skips_the_update_hooks() {
    let manifest = formatdoc! {r#"
        {MANIFEST}
        [scripts]
        pre-update-cmd  = "echo PRE_HOOK"
        post-update-cmd = "echo POST_HOOK"
    "#};
    let registry = registry().await;
    let project = project().manifest(&manifest).lock(LOCK);

    project
        .cmd()
        .args(["update", "--no-scripts", "--registry-url", &registry.uri()])
        .assert()
        .success()
        .stdout(contains("PRE_HOOK").not())
        .stdout(contains("POST_HOOK").not());
}

#[tokio::test]
async fn update_dry_run_skips_the_update_hooks() {
    let manifest = formatdoc! {r#"
        {MANIFEST}
        [scripts]
        pre-update-cmd  = "echo PRE_HOOK"
        post-update-cmd = "echo POST_HOOK"
    "#};
    let registry = registry().await;
    let project = project().manifest(&manifest).lock(LOCK);

    project
        .cmd()
        .args(["update", "--dry-run", "--registry-url", &registry.uri()])
        .assert()
        .success()
        .stdout(contains("PRE_HOOK").not())
        .stdout(contains("POST_HOOK").not());
}

#[tokio::test]
async fn update_fails_without_a_manifest() {
    let project = Project::new();

    project
        .cmd()
        .arg("update")
        .assert()
        .failure()
        .stderr(contains("neither phast.toml nor composer.json found"));
}

#[tokio::test]
async fn update_member_moves_only_what_that_member_requires() {
    let registry = registry().await;
    let project = workspace();

    project
        .cmd()
        .args([
            "update",
            "--member",
            "packages/api",
            "--registry-url",
            &registry.uri(),
        ])
        .assert()
        .success();

    let lock = project.read("phast.lock");
    assert!(
        lock.contains("1.1.0"),
        "the member's package did not move:\n{lock}",
    );
    assert!(
        !lock.contains("2.1.0"),
        "a sibling's package moved anyway:\n{lock}",
    );
}

#[tokio::test]
async fn update_member_yields_to_a_named_package() {
    let registry = registry().await;
    let project = workspace();

    project
        .cmd()
        .args([
            "update",
            "vendor/beta",
            "--member",
            "packages/api",
            "--registry-url",
            &registry.uri(),
        ])
        .assert()
        .success();

    let lock = project.read("phast.lock");
    assert!(
        lock.contains("2.1.0"),
        "the named package did not move:\n{lock}"
    );
    assert!(
        !lock.contains("1.1.0"),
        "the member's own package moved anyway:\n{lock}",
    );
}

#[tokio::test]
async fn update_accepts_a_project_dir() {
    let registry = registry().await;
    let project = project().lock(LOCK);

    project
        .cmd()
        .current_dir(std::env::temp_dir())
        .args([
            "update",
            "--project-dir",
            project.root().to_str().unwrap(),
            "--registry-url",
            &registry.uri(),
        ])
        .assert()
        .success();

    assert!(project.read("phast.lock").contains("1.1.0"));
}
