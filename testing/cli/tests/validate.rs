//! The `phast validate` command surface.
//!
//! The rules themselves are covered in `phast-cli/tests/validate.rs`, which asserts
//! findings directly. This file covers what only the binary can show: exit codes, the
//! `--strict` flag, `--project-dir`, and the messages a caller actually reads. One
//! rule of each severity is enough to drive those; the rest would be duplication.

mod common;

use common::project::Project;

use indoc::indoc;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

const MANIFEST_CLEAN: &str = indoc! {r#"
    [package]
    name = "acme/blog"

    [require]
    php = "^8.4"
"#};

/// `psr/log` in both sections: an error-severity rule.
const MANIFEST_WITH_ERROR: &str = indoc! {r#"
    [package]
    name = "acme/blog"

    [require]
    "psr/log" = "^3.0"

    [require-dev]
    "psr/log" = "^3.0"
"#};

/// A `[sources]` entry for a package nothing requires: a warning-severity rule.
const MANIFEST_WITH_WARNING: &str = indoc! {r#"
    [package]
    name = "acme/blog"

    [require]
    php = "^8.4"

    [[repositories]]
    name = "internal"
    type = "composer"
    url  = "https://packages.acme.example"

    [sources]
    "acme/orphan" = { repository = "internal" }
"#};

const WORKSPACE_ROOT: &str = indoc! {r#"
    [workspace]
    members = ["packages/*"]
"#};

/// A project holding the given manifest.
fn project(manifest: &str) -> Project {
    Project::new().manifest(manifest)
}

/// A workspace whose root manifest is clean and whose one member is not, so `--member`
/// is observable: without it the root passes and the member's error goes unreported.
fn workspace() -> Project {
    Project::new()
        .manifest(WORKSPACE_ROOT)
        .write("packages/api/phast.toml", MANIFEST_WITH_ERROR)
}

#[test]
fn validate_accepts_a_clean_manifest() {
    let project = project(MANIFEST_CLEAN);

    project
        .cmd()
        .arg("validate")
        .assert()
        .success()
        .stdout(contains("phast.toml is valid."));
}

#[test]
fn validate_fails_on_an_error_and_names_it() {
    let project = project(MANIFEST_WITH_ERROR);

    project
        .cmd()
        .arg("validate")
        .assert()
        .failure()
        .stderr(contains("'psr/log' is in both [require] and [require-dev]"))
        .stderr(contains("1 error(s) found"));
}

#[test]
fn validate_keeps_a_finding_and_its_hint_together() {
    let project = project(MANIFEST_WITH_ERROR);

    project
        .cmd()
        .arg("validate")
        .assert()
        .failure()
        .stderr(contains(
            "'psr/log' is in both [require] and [require-dev]\n  hint: keep it in one section",
        ));
}

#[test]
fn validate_reports_a_warning_without_failing() {
    let project = project(MANIFEST_WITH_WARNING);

    project
        .cmd()
        .arg("validate")
        .assert()
        .success()
        .stderr(contains(
            "[sources] 'acme/orphan' is not in [require] or [require-dev]",
        ))
        .stdout(contains("phast.toml is valid (1 warning(s))."));
}

#[test]
fn validate_strict_turns_a_warning_into_a_failure() {
    let project = project(MANIFEST_WITH_WARNING);

    project
        .cmd()
        .args(["validate", "--strict"])
        .assert()
        .failure()
        .stderr(contains("1 warning(s) found"));
}

#[test]
fn validate_strict_still_accepts_a_clean_manifest() {
    let project = project(MANIFEST_CLEAN);

    project
        .cmd()
        .args(["validate", "--strict"])
        .assert()
        .success()
        .stdout(contains("phast.toml is valid."));
}

#[test]
fn validate_accepts_a_project_dir() {
    let project = project(MANIFEST_CLEAN);
    let elsewhere = common::temp_dir();

    project
        .cmd()
        .args(["validate", "--project-dir"])
        .arg(project.root())
        .current_dir(elsewhere.path())
        .assert()
        .success()
        .stdout(contains("phast.toml is valid."));
}

#[test]
fn validate_fails_without_a_manifest() {
    let project = Project::new();

    project
        .cmd()
        .arg("validate")
        .assert()
        .failure()
        .stderr(contains("phast.toml not found"));
}

#[test]
fn validate_fails_on_an_unparseable_manifest() {
    let project = project("this is not toml [[[");

    project
        .cmd()
        .arg("validate")
        .assert()
        .failure()
        .stderr(contains("phast.toml is invalid"));
}

#[test]
fn validate_member_checks_that_members_manifest() {
    let workspace = workspace();

    workspace
        .cmd()
        .args(["validate", "--member", "packages/api"])
        .assert()
        .failure()
        .stderr(contains("'psr/log' is in both [require] and [require-dev]"));
}

#[test]
fn validate_covers_every_member_by_default() {
    let workspace = workspace();

    workspace
        .cmd()
        .arg("validate")
        .assert()
        .failure()
        .stderr(contains(
            "acme/blog: 'psr/log' is in both [require] and [require-dev]",
        ));
}

#[test]
fn validate_reports_a_missing_manifest_before_looking_for_a_workspace() {
    let project = Project::new();

    project
        .cmd()
        .args(["validate", "--member", "packages/api"])
        .assert()
        .failure()
        .stderr(contains("phast.toml not found"));
}

#[test]
fn validate_member_expands_an_inherited_constraint() {
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
                name = "acme/api"

                [require]
                "psr/log" = { workspace = true }
            "#},
        );

    project
        .cmd()
        .args(["validate", "--member", "packages/api"])
        .assert()
        .success()
        .stdout(contains("phast.toml is valid"));
}

#[test]
fn validate_reports_a_defect_in_every_member() {
    let project = Project::new()
        .manifest(WORKSPACE_ROOT)
        .write(
            "packages/api/phast.toml",
            indoc! {r#"
                [package]
                name = "acme/api"

                [require]
                "psr/log" = "^3.0"

                [require-dev]
                "psr/log" = "^3.0"
            "#},
        )
        .write(
            "packages/web/phast.toml",
            indoc! {r#"
                [package]
                name = "acme/web"

                [require]
                "acme/x" = "*"

                [sources]
                "acme/x" = { repository = "does-not-exist" }
            "#},
        );

    project
        .cmd()
        .arg("validate")
        .assert()
        .failure()
        .stderr(contains("acme/api: 'psr/log' is in both"))
        .stderr(contains("acme/web: [sources] 'acme/x' names repository"))
        .stderr(contains("2 error(s) found"));
}

#[test]
fn validate_reports_a_defect_at_the_workspace_root() {
    let project = Project::new()
        .manifest(indoc! {r#"
            [workspace]
            members = ["packages/*"]

            [sources]
            "acme/x" = { repository = "does-not-exist" }
        "#})
        .write("packages/api/phast.toml", MANIFEST_CLEAN);

    project
        .cmd()
        .arg("validate")
        .assert()
        .failure()
        .stderr(contains("names repository 'does-not-exist'"));
}

#[test]
fn validate_strict_fails_on_a_warning_in_a_member() {
    let project = Project::new()
        .manifest(WORKSPACE_ROOT)
        .write("packages/api/phast.toml", MANIFEST_WITH_WARNING);

    project
        .cmd()
        .args(["validate", "--strict"])
        .assert()
        .failure()
        .stderr(contains("1 warning(s) found"));
}

#[test]
fn validate_does_not_name_a_member_in_a_standalone_project() {
    let project = project(MANIFEST_WITH_ERROR);

    project
        .cmd()
        .arg("validate")
        .assert()
        .failure()
        .stderr(contains("'psr/log' is in both"))
        .stderr(contains("acme/blog:").not());
}

/// Narrowing means narrowing: a sibling's defect is not this member's problem.
#[test]
fn validate_member_ignores_a_sibling_defect() {
    let project = Project::new()
        .manifest(WORKSPACE_ROOT)
        .write("packages/api/phast.toml", MANIFEST_CLEAN)
        .write("packages/web/phast.toml", MANIFEST_WITH_ERROR);

    project
        .cmd()
        .args(["validate", "--member", "packages/api"])
        .assert()
        .success()
        .stdout(contains("phast.toml is valid"));
}

#[test]
fn validate_member_outside_a_workspace_errors() {
    let project = project(MANIFEST_CLEAN);

    project
        .cmd()
        .args(["validate", "--member", "packages/api"])
        .assert()
        .failure()
        .stderr(contains("--member requires a workspace"));
}

/// A project that cannot be loaded cannot be linted, and saying so is the point: this used
/// to fall through to only the root manifest and report the workspace as valid.
#[test]
fn validate_fails_when_the_workspace_cannot_be_loaded() {
    let project = Project::new()
        .manifest(WORKSPACE_ROOT)
        .write(
            "packages/api/phast.toml",
            indoc! {r#"
                [package]
                name = "acme/api"

                [require]
                "psr/log" = "^3.0"
            "#},
        )
        .write(
            "packages/web/phast.toml",
            indoc! {r#"
                [package]
                name = "acme/web"

                [require]
                "psr/log" = "^2.0"
            "#},
        );

    project
        .cmd()
        .arg("validate")
        .assert()
        .failure()
        .stderr(contains("workspace dependency conflict for \"psr/log\""));
}

/// `--project-dir` pointed at a member walks up to the root, so the member is checked
/// expanded rather than read on its own.
#[test]
fn validate_accepts_a_project_dir_naming_a_member() {
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
                name = "acme/api"

                [require]
                "psr/log" = { workspace = true }
            "#},
        );
    let elsewhere = common::temp_dir();

    project
        .cmd()
        .args(["validate", "--project-dir"])
        .arg(project.root().join("packages/api"))
        .current_dir(elsewhere.path())
        .assert()
        .success()
        .stdout(contains("phast.toml is valid"));
}

/// A project with several manifests is spoken of collectively; one is named. The singular
/// cases are covered above, standalone and narrowed by `--member`.
#[test]
fn validate_reports_a_clean_workspace_collectively() {
    let project = Project::new()
        .manifest(WORKSPACE_ROOT)
        .write("packages/api/phast.toml", MANIFEST_CLEAN);

    project
        .cmd()
        .arg("validate")
        .assert()
        .success()
        .stdout(contains("Every manifest is valid."));
}
