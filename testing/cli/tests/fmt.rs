mod common;

use common::project::Project;

use indoc::indoc;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

/// Already correctly formatted: php first, vendor packages alphabetically.
const MANIFEST_FORMATTED: &str = indoc! {r#"
    [package]
    name = "acme/test"
    type = "project"

    [require]
    php = "^8.2"
    "vendor/package-a" = "^1.0"
    "vendor/package-b" = "^2.0"

    [config]
    sort-packages = true
"#};

/// Packages in wrong order: vendor packages before php.
const MANIFEST_UNSORTED: &str = indoc! {r#"
    [package]
    name = "acme/test"
    type = "project"

    [require]
    "vendor/package-a" = "^1.0"
    "vendor/package-b" = "^2.0"
    php = "^8.2"

    [config]
    sort-packages = true
"#};

/// A project holding the given manifest.
fn project(manifest: &str) -> Project {
    Project::new().manifest(manifest)
}

const WORKSPACE_ROOT: &str = indoc! {r#"
    [workspace]
    members = ["packages/*"]
"#};

/// A workspace whose root is formatted and whose two members are not, so coverage cannot
/// stop at the root or at the first member.
fn workspace() -> Project {
    Project::new()
        .manifest(WORKSPACE_ROOT)
        .write("packages/api/phast.toml", MANIFEST_UNSORTED)
        .write("packages/web/phast.toml", MANIFEST_UNSORTED)
}

#[test]
fn fmt_formats_in_place() {
    let project = project(MANIFEST_UNSORTED);

    project.cmd().arg("fmt").assert().success();

    let result: toml::Value = toml::from_str(&project.read("phast.toml")).unwrap();

    let keys: Vec<&str> = result["require"]
        .as_table()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();

    assert_eq!(keys[0], "php", "php should be sorted first");
}

#[test]
fn fmt_is_idempotent() {
    let project = project(MANIFEST_FORMATTED);

    project.cmd().arg("fmt").assert().success();

    let after = project.read("phast.toml");
    assert_eq!(after, MANIFEST_FORMATTED);
}

#[test]
fn fmt_check_passes_on_formatted_file() {
    let project = project(MANIFEST_FORMATTED);

    project.cmd().args(["fmt", "--check"]).assert().success();
}

#[test]
fn fmt_check_fails_on_unformatted_file() {
    let project = project(MANIFEST_UNSORTED);

    project.cmd().args(["fmt", "--check"]).assert().failure();
}

#[test]
fn fmt_check_does_not_write() {
    let project = project(MANIFEST_UNSORTED);

    project.cmd().args(["fmt", "--check"]).assert().failure();

    let after = project.read("phast.toml");
    assert_eq!(after, MANIFEST_UNSORTED, "--check must not write the file");
}

#[test]
fn fmt_fails_without_phast_toml() {
    let project = Project::new();

    project.cmd().arg("fmt").assert().failure();
}

#[test]
fn fmt_accepts_project_dir() {
    let project = project(MANIFEST_UNSORTED);

    project
        .cmd()
        .args(["fmt", "--project-dir", project.root().to_str().unwrap()])
        .assert()
        .success();

    let result: toml::Value = toml::from_str(&project.read("phast.toml")).unwrap();

    let keys: Vec<&str> = result["require"]
        .as_table()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();

    assert_eq!(keys[0], "php");
}

#[test]
fn fmt_reports_a_missing_manifest_before_looking_for_a_workspace() {
    Project::new()
        .cmd()
        .arg("fmt")
        .assert()
        .failure()
        .stderr(contains("phast.toml not found"));
}

/// The gate this closed: a workspace root is formatted while every member is not, and
/// `--check` used to look at only the root and exit zero.
#[test]
fn fmt_check_fails_on_an_unformatted_member() {
    let workspace = workspace();

    workspace
        .cmd()
        .args(["fmt", "--check"])
        .assert()
        .failure()
        .stderr(contains("2 manifests are not formatted"));
}

#[test]
fn fmt_formats_every_member() {
    let project = workspace();

    project.cmd().arg("fmt").assert().success();

    assert_eq!(project.read("packages/api/phast.toml"), MANIFEST_FORMATTED);
    assert_eq!(project.read("packages/web/phast.toml"), MANIFEST_FORMATTED);
}

/// Members are named as `validate` names them, by `[package].name`, and the root by `root`.
#[test]
fn fmt_names_each_manifest_it_rewrites() {
    let project = Project::new()
        .manifest(indoc! {r#"
            [workspace]
            members=["packages/*"]
        "#})
        .write("packages/api/phast.toml", MANIFEST_UNSORTED);

    project
        .cmd()
        .arg("fmt")
        .assert()
        .success()
        .stdout(contains("root"))
        .stdout(contains("acme/test"));
}

/// One file needs no distinguishing, so a standalone project is reported without a name.
#[test]
fn fmt_names_nothing_in_a_standalone_project() {
    let project = project(MANIFEST_UNSORTED);

    project
        .cmd()
        .arg("fmt")
        .assert()
        .success()
        .stdout(contains("phast.toml formatted."))
        .stdout(contains("acme/test").not());
}

/// A member that inherits a constraint is formatted, not rejected: formatting is textual, so
/// `{ workspace = true }` never has to resolve.
#[test]
fn fmt_formats_a_member_that_inherits_a_constraint() {
    let project = Project::new()
        .manifest(indoc! {r#"
            [workspace]
            members = ["packages/*"]

            [workspace.dependencies]
            "psr/log" = "^3.0"
        "#})
        .write(
            "packages/api/phast.toml",
            indoc! {r#"
                [package]
                name="acme/api"

                [require]
                "psr/log" = { workspace = true }
            "#},
        );

    project.cmd().arg("fmt").assert().success();

    assert!(
        project
            .read("packages/api/phast.toml")
            .contains(r#""psr/log" = { workspace = true }"#),
        "the inherited constraint did not survive formatting",
    );
}

#[test]
fn fmt_member_leaves_a_sibling_alone() {
    let project = workspace();

    project
        .cmd()
        .args(["fmt", "--member", "packages/api"])
        .assert()
        .success();

    assert_eq!(project.read("packages/api/phast.toml"), MANIFEST_FORMATTED);
    assert_eq!(project.read("packages/web/phast.toml"), MANIFEST_UNSORTED);
}

#[test]
fn fmt_check_writes_nothing_for_members() {
    let project = workspace();

    project.cmd().args(["fmt", "--check"]).assert().failure();

    assert_eq!(project.read("packages/api/phast.toml"), MANIFEST_UNSORTED);
    assert_eq!(project.read("packages/web/phast.toml"), MANIFEST_UNSORTED);
}
