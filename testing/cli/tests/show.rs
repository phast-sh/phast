//! The `phast show` command surface.
//!
//! `show` reports versions from `phast.lock`, and reads the manifest only to locate
//! the workspace root and to narrow the listing, so every test here writes both.

mod common;

use common::project::Project;

use indoc::indoc;
use predicates::prelude::*;
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

const WORKSPACE: &str = indoc! {r#"
    [workspace]
    members = ["packages/*"]
"#};

fn project() -> Project {
    Project::new().manifest(MANIFEST).lock(LOCK)
}

/// A workspace whose one member requires `vendor/alpha` and nothing else. `vendor/beta`
/// is locked as its transitive dependency, and `vendor/tool` as a dev requirement.
fn workspace() -> Project {
    Project::new()
        .manifest(WORKSPACE)
        .lock(indoc! {r#"
            version      = 1
            content-hash = "sha256:test"

            [metadata]
            generated-by = "phast 0.1.0"
            generated-at = "2026-01-01T00:00:00+00:00"

            [[packages]]
            name    = "vendor/alpha"
            version = "1.0.0"

            [[packages]]
            name    = "vendor/beta"
            version = "2.0.0"

            [[packages-dev]]
            name    = "vendor/tool"
            version = "3.0.0"
        "#})
        .write(
            "packages/api/phast.toml",
            indoc! {r#"
                [package]
                name = "acme/api"

                [require]
                "vendor/alpha" = "^1.0"

                [require-dev]
                "vendor/tool" = "^3.0"
            "#},
        )
}

#[test]
fn show_lists_every_locked_package() {
    let project = project();

    project
        .cmd()
        .arg("show")
        .assert()
        .success()
        .stdout(contains("vendor/alpha"))
        .stdout(contains("1.0.0"))
        .stdout(contains("vendor/beta"))
        .stdout(contains("2.0.0"));
}

#[test]
fn show_details_a_named_package() {
    let project = project();

    project
        .cmd()
        .args(["show", "vendor/alpha"])
        .assert()
        .success()
        .stdout(contains("name     : vendor/alpha"))
        .stdout(contains("version  : 1.0.0"))
        .stdout(contains("requires"))
        .stdout(contains("vendor/beta"));
}

#[test]
fn show_details_several_packages_in_order() {
    let project = project();

    let output = project
        .cmd()
        .args(["show", "vendor/beta", "vendor/alpha"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    let beta = stdout.find("vendor/beta").expect("beta is missing");
    let alpha = stdout.find("vendor/alpha").expect("alpha is missing");

    assert!(beta < alpha, "packages should print in the order given");
}

#[test]
fn show_ignores_a_constraint_on_the_argument() {
    let project = project();

    project
        .cmd()
        .args(["show", "vendor/alpha:^1.0"])
        .assert()
        .success()
        .stdout(contains("name     : vendor/alpha"));
}

#[test]
fn show_emits_json_for_every_package() {
    let project = project();

    let output = project
        .cmd()
        .args(["show", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("stdout is not JSON");
    let packages = json["packages"].as_array().expect("no packages array");

    assert_eq!(packages.len(), 2);
    assert_eq!(packages[0]["name"].as_str(), Some("vendor/alpha"));
    assert_eq!(packages[0]["version"].as_str(), Some("1.0.0"));
    assert!(json["packages-dev"].is_array());
}

#[test]
fn show_emits_json_for_one_package() {
    let project = project();

    let output = project
        .cmd()
        .args(["show", "vendor/alpha", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("stdout is not JSON");

    assert_eq!(json["package"]["name"].as_str(), Some("vendor/alpha"));
    assert_eq!(
        json["package"]["require"]["vendor/beta"].as_str(),
        Some("^2.0")
    );
    assert!(json.get("packages").is_none());
}

#[test]
fn show_accepts_a_project_dir() {
    let project = project();
    let elsewhere = common::temp_dir();

    project
        .cmd()
        .args(["show", "--project-dir"])
        .arg(project.root())
        .current_dir(elsewhere.path())
        .assert()
        .success()
        .stdout(contains("vendor/alpha"));
}

#[test]
fn show_fails_for_a_package_not_in_the_lock() {
    let project = project();

    project
        .cmd()
        .args(["show", "vendor/missing"])
        .assert()
        .failure()
        .stderr(contains("`vendor/missing` is not in phast.lock"));
}

#[test]
fn show_fails_for_an_invalid_package_name() {
    let project = project();

    project
        .cmd()
        .args(["show", "notapackage"])
        .assert()
        .failure()
        .stderr(contains("invalid package name 'notapackage'"));
}

#[test]
fn show_fails_without_a_lock() {
    let project = Project::new().manifest(MANIFEST);

    project
        .cmd()
        .arg("show")
        .assert()
        .failure()
        .stderr(contains("phast.lock not found"));
}

#[test]
fn show_narrows_the_listing_to_one_member() {
    workspace()
        .cmd()
        .args(["show", "--member", "packages/api"])
        .assert()
        .success()
        .stdout(contains("vendor/alpha").and(contains("vendor/beta").not()));
}

#[test]
fn show_lists_every_locked_package_at_the_workspace_root() {
    workspace()
        .cmd()
        .arg("show")
        .assert()
        .success()
        .stdout(contains("vendor/alpha").and(contains("vendor/beta")));
}

#[test]
fn show_omits_dev_packages_when_asked() {
    workspace()
        .cmd()
        .args(["show", "--no-dev"])
        .assert()
        .success()
        .stdout(contains("vendor/tool").not());
}

#[test]
fn show_details_a_package_outside_the_member_scope() {
    workspace()
        .cmd()
        .args(["show", "vendor/beta", "--member", "packages/api"])
        .assert()
        .success()
        .stdout(contains("vendor/beta"));
}
