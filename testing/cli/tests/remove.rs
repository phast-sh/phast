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
    "vendor/package-a" = "^1.0"
    "vendor/package-b" = "^2.0"

    [require-dev]
    "vendor/package-c" = "^3.0"
    "vendor/package-d" = "^4.0"
"#};

const MANIFEST_WORKSPACE_ROOT: &str = indoc! {r#"
    [workspace]
    members = ["packages/*"]

    [workspace.dependencies]
    "vendor/package-a" = "^1.0"
"#};

const MANIFEST_INHERITING: &str = indoc! {r#"
    [package]
    name    = "acme/api"
    version = "1.0.0"

    [require]
    "vendor/package-a" = { workspace = true }
    "vendor/package-b" = "^2.0"
"#};

/// A project with two requires and two dev requires.
fn project() -> Project {
    Project::new().manifest(MANIFEST)
}

/// A registry serving `vendor/package-a` and `vendor/package-c` at one version each.
async fn registry() -> MockRegistry {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/package-a", &["1.0.0"], json!({}))
        .await;
    registry
        .mount("vendor/package-c", &["3.0.0"], json!({}))
        .await;
    registry
}

// ---------------------------------------------------------------------------
// Editing the manifest
// ---------------------------------------------------------------------------

#[test]
fn remove_drops_a_package_from_require() {
    let project = project();

    project
        .cmd()
        .args(["remove", "vendor/package-a", "--no-install"])
        .assert()
        .success();

    let toml = project.to_toml();
    assert!(toml["require"].get("vendor/package-a").is_none());
    assert_eq!(toml["require"]["vendor/package-b"].as_str(), Some("^2.0"));
}

#[test]
fn remove_drops_a_package_from_require_dev() {
    let project = project();

    project
        .cmd()
        .args(["remove", "--dev", "vendor/package-c", "--no-install"])
        .assert()
        .success();

    let toml = project.to_toml();
    assert!(toml["require-dev"].get("vendor/package-c").is_none());
    assert_eq!(
        toml["require-dev"]["vendor/package-d"].as_str(),
        Some("^4.0")
    );
}

#[test]
fn remove_drops_a_section_it_empties() {
    let project = project();

    project
        .cmd()
        .args(["remove", "--dev", "vendor/package-c", "vendor/package-d"])
        .arg("--no-install")
        .assert()
        .success();

    assert!(project.to_toml().get("require-dev").is_none());
}

#[test]
fn remove_drops_an_inherited_dependency() {
    let project = Project::new()
        .manifest(MANIFEST_WORKSPACE_ROOT)
        .write("packages/api/phast.toml", MANIFEST_INHERITING);

    project
        .cmd()
        .args(["remove", "--member", "packages/api"])
        .args(["vendor/package-a", "--no-install"])
        .assert()
        .success();

    let member: toml::Value =
        toml::from_str(&project.read("packages/api/phast.toml")).expect("member is valid TOML");
    assert!(member["require"].get("vendor/package-a").is_none());
    assert_eq!(member["require"]["vendor/package-b"].as_str(), Some("^2.0"));
}

// ---------------------------------------------------------------------------
// Refused requests
// ---------------------------------------------------------------------------

#[test]
fn remove_fails_for_an_undeclared_package() {
    let project = project();

    project
        .cmd()
        .args(["remove", "vendor/nonexistent", "--no-install"])
        .assert()
        .failure();
}

#[test]
fn remove_fails_for_a_package_in_the_other_section() {
    let project = project();

    project
        .cmd()
        .args(["remove", "--dev", "vendor/package-a", "--no-install"])
        .assert()
        .failure();
}

#[test]
fn remove_at_a_workspace_root_names_the_member_flag() {
    let project = Project::new()
        .manifest(MANIFEST_WORKSPACE_ROOT)
        .write("packages/api/phast.toml", MANIFEST_INHERITING);
    let manifest = project.read("phast.toml");

    project
        .cmd()
        .args(["remove", "vendor/package-a", "--no-install"])
        .assert()
        .failure()
        .stderr(contains("--member"))
        .stderr(contains("--dev").not());

    assert_eq!(
        project.read("phast.toml"),
        manifest,
        "the root was written to",
    );
}

#[test]
fn remove_fails_without_phast_toml() {
    let project = Project::new();

    project
        .cmd()
        .args(["remove", "vendor/package-a", "--no-install"])
        .assert()
        .failure();
}

#[test]
fn remove_reports_a_missing_manifest_before_looking_for_a_workspace() {
    Project::new()
        .cmd()
        .args(["remove", "psr/log"])
        .assert()
        .failure()
        .stderr(contains("phast.toml not found"));
}

// ---------------------------------------------------------------------------
// The install that follows
// ---------------------------------------------------------------------------

#[tokio::test]
async fn remove_installs_against_the_edited_manifest() {
    let registry = registry().await;
    let project = Project::new().manifest(&formatdoc! {r#"
        [package]
        name = "acme/my-project"
        type = "project"

        [require]
        "vendor/package-a" = "^1.0"

        [require-dev]
        "vendor/package-c" = "^3.0"

        [[repositories]]
        type = "composer"
        url  = "{uri}"
    "#, uri = registry.uri()});

    project.cmd().arg("install").assert().success();
    assert!(project.read("phast.lock").contains("vendor/package-c"));

    project
        .cmd()
        .args(["remove", "--dev", "vendor/package-c"])
        .assert()
        .success()
        .stdout(contains("Lock file is up to date").not());

    assert!(
        !project.read("phast.lock").contains("vendor/package-c"),
        "the removed package is still locked",
    );
    assert!(project.read("phast.lock").contains("vendor/package-a"));
}
