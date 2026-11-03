mod common;

use common::project::Project;
use common::registry::MockRegistry;

use indoc::indoc;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::json;

const MANIFEST: &str = indoc! {r#"
    [package]
    name = "acme/my-project"
    type = "project"

    [require]
    "vendor/alpha" = "^1.0"
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

/// A project requiring one package, with both install hooks echoing a marker.
const MANIFEST_WITH_HOOKS: &str = indoc! {r#"
    [package]
    name = "acme/my-project"
    type = "project"

    [require]
    "vendor/alpha" = "^1.0"

    [scripts]
    pre-install-cmd  = "echo PRE_HOOK"
    post-install-cmd = "echo POST_HOOK"
"#};

/// A project requiring one package.
fn project() -> Project {
    Project::new().manifest(MANIFEST)
}

/// A workspace whose two members need different packages.
///
/// `install --member` downloads one member's closure; the point of the assertions below is
/// which of `vendor/alpha` and `vendor/beta` reaches `vendor/`.
#[rustfmt::skip]
fn workspace() -> Project {
    Project::new()
        .manifest(indoc! {r#"
            [workspace]
            members = ["packages/*"]
        "#})
        .write("packages/api/phast.toml", indoc! {r#"
            [package]
            name    = "acme/api"
            version = "1.0.0"

            [require]
            "vendor/alpha" = "^1.0"
        "#})
        .write("packages/logger/phast.toml", indoc! {r#"
            [package]
            name    = "acme/logger"
            version = "1.0.0"

            [require]
            "vendor/beta" = "^2.0"
        "#})
}

/// A registry serving `vendor/alpha` 1.0.0 with no dependencies of its own.
///
/// The single package every install test in this file resolves against.
async fn registry() -> MockRegistry {
    let registry = MockRegistry::start().await;
    registry.mount("vendor/alpha", &["1.0.0"], json!({})).await;
    registry
}

/// A registry serving what either member might ask for.
async fn workspace_registry() -> MockRegistry {
    let registry = MockRegistry::start().await;
    registry.mount("vendor/alpha", &["1.0.0"], json!({})).await;
    registry.mount("vendor/beta", &["2.0.0"], json!({})).await;
    registry
}

#[tokio::test]
async fn install_dry_run_resolves_without_downloading() {
    let registry = registry().await;

    let project = project();

    project
        .cmd()
        .args(["install", "--dry-run", "--registry-url", &registry.uri()])
        .assert()
        .success()
        .stdout(contains("Would install vendor/alpha"));

    assert!(!project.root().join("phast.lock").exists());
    assert!(!project.root().join("vendor").exists());
}

#[tokio::test]
async fn install_dry_run_skips_the_install_hooks() {
    let registry = registry().await;
    let project = Project::new().manifest(MANIFEST_WITH_HOOKS);

    project
        .cmd()
        .args(["install", "--dry-run", "--registry-url", &registry.uri()])
        .assert()
        .success()
        .stdout(contains("PRE_HOOK").not())
        .stdout(contains("POST_HOOK").not());
}

#[tokio::test]
async fn install_dry_run_from_a_fresh_lock_writes_nothing() {
    let registry = registry().await;
    let project = Project::new().manifest(MANIFEST_WITH_HOOKS);

    project
        .cmd()
        .args(["install", "--no-scripts", "--registry-url", &registry.uri()])
        .assert()
        .success();
    std::fs::remove_dir_all(project.root().join("vendor")).unwrap();

    project
        .cmd()
        .args(["install", "--dry-run", "--registry-url", &registry.uri()])
        .assert()
        .success()
        .stdout(contains("Would install vendor/alpha"))
        .stdout(contains("PRE_HOOK").not())
        .stdout(contains("POST_HOOK").not());

    assert!(
        !project.root().join("vendor").exists(),
        "a dry run installed into vendor/",
    );
}

#[tokio::test]
async fn install_writes_lock_and_vendor() {
    let registry = registry().await;

    let project = project();

    project
        .cmd()
        .args(["install", "--registry-url", &registry.uri()])
        .assert()
        .success();

    assert!(project.root().join("phast.lock").exists());
    assert!(project.root().join("vendor/vendor/alpha").is_dir());
    assert!(project.root().join("vendor/autoload.php").exists());
}

#[tokio::test]
async fn install_from_fresh_lock_skips_resolution() {
    let registry = registry().await;

    let project = project();

    // First install: creates phast.lock and vendor/.
    project
        .cmd()
        .args(["install", "--registry-url", &registry.uri()])
        .assert()
        .success();

    // Second install: lock is fresh, vendor/ is populated -- nothing to do.
    project
        .cmd()
        .args(["install", "--registry-url", &registry.uri()])
        .assert()
        .success()
        .stdout(contains("Lock file is up to date"))
        .stdout(contains("Nothing to install"));
}

#[tokio::test]
async fn install_no_dev_still_locks_the_dev_packages() {
    let registry = workspace_registry().await;
    let project = Project::new().manifest(MANIFEST_WITH_DEV);

    project
        .cmd()
        .args(["install", "--no-dev", "--registry-url", &registry.uri()])
        .assert()
        .success();

    assert!(
        project.read("phast.lock").contains("vendor/beta"),
        "--no-dev left the dev package out of the lock",
    );
    assert!(!project.root().join("vendor/vendor/beta").exists());

    project
        .cmd()
        .args(["install", "--registry-url", &registry.uri()])
        .assert()
        .success();

    assert!(
        project.root().join("vendor/vendor/beta").is_dir(),
        "a plain install after --no-dev never installed the dev package",
    );
}

// ---------------------------------------------------------------------------
// Narrowing to a member
// ---------------------------------------------------------------------------

#[tokio::test]
async fn install_member_downloads_only_that_members_closure() {
    let registry = workspace_registry().await;
    let project = workspace();

    project
        .cmd()
        .args(["install", "--member", "packages/api"])
        .args(["--registry-url", &registry.uri()])
        .assert()
        .success();

    assert!(project.root().join("vendor/vendor/alpha").is_dir());
    assert!(
        !project.root().join("vendor/vendor/beta").is_dir(),
        "a sibling's package was downloaded despite --member",
    );
}

/// Standing in a member scopes the install, the same as naming it. Before the member
/// resolution was shared, `install` was the one command that ignored where it was run.
#[tokio::test]
async fn install_from_inside_a_member_scopes_to_it() {
    let registry = workspace_registry().await;
    let project = workspace();

    project
        .cmd()
        .args(["install", "--registry-url", &registry.uri()])
        .current_dir(project.root().join("packages/api"))
        .assert()
        .success();

    assert!(project.root().join("vendor/vendor/alpha").is_dir());
    assert!(
        !project.root().join("vendor/vendor/beta").is_dir(),
        "running inside a member installed the whole workspace",
    );
}

/// The one thing `install` asks of a member beyond existing: a lock closure resolves on a
/// package identity, so a member without `[package].name` has nothing to scope to.
#[tokio::test]
async fn install_member_without_a_name_is_refused() {
    let registry = workspace_registry().await;
    let project = workspace().write(
        "packages/anon/phast.toml",
        indoc! {r#"
            [package]
            version = "1.0.0"
        "#},
    );

    project
        .cmd()
        .args(["install", "--member", "packages/anon"])
        .args(["--registry-url", &registry.uri()])
        .assert()
        .failure()
        .stderr(contains("no [package].name"));
}
