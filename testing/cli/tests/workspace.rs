mod common;

use common::project::Project;

use indoc::{formatdoc, indoc};
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const MANIFEST_ROOT: &str = indoc! {r#"
    [workspace]
    members = ["packages/*"]
"#};

const MANIFEST_STANDALONE: &str = indoc! {r#"
    [package]
    name    = "acme/app"
    version = "1.0.0"
"#};

const MANIFEST_API: &str = indoc! {r#"
    [package]
    name    = "acme/api"
    version = "1.0.0"

    [require]
    "vendor/alpha" = "^1.0"
"#};

const MANIFEST_LOGGER: &str = indoc! {r#"
    [package]
    name    = "acme/logger"
    version = "1.0.0"

    [require]
    "vendor/beta" = "^2.0"
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

    [[packages]]
    name    = "vendor/beta"
    version = "2.0.0"
"#};

/// A workspace root with two members.
fn project() -> Project {
    Project::new()
        .manifest(MANIFEST_ROOT)
        .lock(LOCK)
        .write("packages/api/phast.toml", MANIFEST_API)
        .write("packages/logger/phast.toml", MANIFEST_LOGGER)
}

/// A member asking for a different constraint than `MANIFEST_API` on the same package.
const MANIFEST_LOGGER_CONFLICTING: &str = indoc! {r#"
    [package]
    name    = "acme/logger"
    version = "1.0.0"

    [require]
    "vendor/alpha" = "^1.1"
"#};

/// Two members disagreeing over the same package, with `shared` dependencies
/// deciding whether the root pins it.
#[rustfmt::skip]
fn conflicting_project(shared: &str) -> Project {
    Project::new()
        .manifest(
            &formatdoc! {r#"
                [workspace]
                members = ["packages/*"]

                {shared}
            "#}
        )
        .write("packages/api/phast.toml", MANIFEST_API)
        .write("packages/logger/phast.toml", MANIFEST_LOGGER_CONFLICTING)
}

/// A root declaring the shared constraint, and scripts at both levels.
const MANIFEST_ROOT_SHARED: &str = indoc! {r#"
    [workspace]
    members = ["packages/*"]

    [workspace.dependencies]
    "vendor/alpha" = "^1.0"

    [scripts]
    greet              = "echo ROOT_SCRIPT"
    post-autoload-dump = "echo ROOT_HOOK"
"#};

/// A member that takes its constraint from the root instead of pinning one.
const MANIFEST_INHERITING: &str = indoc! {r#"
    [package]
    name    = "acme/api"
    version = "1.0.0"

    [require]
    "vendor/alpha" = { workspace = true }

    [scripts]
    check              = "echo MEMBER_SCRIPT"
    post-autoload-dump = "echo MEMBER_HOOK"
"#};

/// A workspace whose only member inherits its constraint.
///
/// `{ workspace = true }` is the whole point: the member manifest on disk cannot be read
/// on its own, so every command here has to go through the workspace that expands it.
fn inheriting_project() -> Project {
    Project::new()
        .manifest(MANIFEST_ROOT_SHARED)
        .lock(LOCK)
        .write("packages/api/phast.toml", MANIFEST_INHERITING)
}

/// Two members requiring the same package at different constraints, plus one each.
#[rustfmt::skip]
fn attributing_project() -> Project {
    Project::new()
        .manifest(MANIFEST_ROOT_SHARED)
        .lock(LOCK)
        .write("packages/api/phast.toml",
            indoc! {r#"
                [package]
                name    = "acme/api"
                version = "1.0.0"

                [require]
                "vendor/alpha" = { workspace = true }
                "vendor/beta"  = "^2.0"
            "#}
        )
        .write("packages/logger/phast.toml",
            indoc! {r#"
                [package]
                name    = "acme/logger"
                version = "1.0.0"

                [require]
                "vendor/beta" = "^2.0"
            "#}
        )
}

// ---------------------------------------------------------------------------
// phast workspace
// ---------------------------------------------------------------------------

#[test]
fn workspace_lists_members() {
    let project = project();

    project.cmd().arg("workspace").assert().success();
}

#[test]
fn workspace_output_includes_member_names() {
    let project = project();

    project
        .cmd()
        .arg("workspace")
        .assert()
        .success()
        .stdout(contains("api"))
        .stdout(contains("logger"));
}

#[test]
fn workspace_accepts_a_project_dir() {
    let project = project();

    project
        .cmd()
        .current_dir(std::env::temp_dir())
        .args([
            "workspace",
            "--project-dir",
            project.root().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(contains("api"))
        .stdout(contains("logger"));
}

#[test]
fn workspace_fails_in_standalone_project() {
    let project = Project::new().manifest(MANIFEST_STANDALONE);

    project.cmd().arg("workspace").assert().failure();
}

#[test]
fn workspace_fails_without_manifest() {
    let project = Project::new();

    project.cmd().arg("workspace").assert().failure();
}

// ---------------------------------------------------------------------------
// phast why --member
// ---------------------------------------------------------------------------

#[test]
fn why_member_flag_is_accepted() {
    let project = project();

    let output = project
        .cmd()
        .args(["why", "vendor/alpha", "--member", "packages/api"])
        .assert()
        .get_output()
        .clone();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unexpected argument"),
        "flag was rejected: {stderr}"
    );
}

#[test]
fn why_with_member_shows_requirer() {
    let project = project();

    // vendor/alpha is in acme/api's [require], so acme/api is the requirer. The root
    // cannot be: one carrying [require] is refused at load.
    project
        .cmd()
        .args(["why", "vendor/alpha", "--member", "packages/api"])
        .assert()
        .success()
        .stdout(contains("acme/api"))
        .stdout(contains("root").not());
}

#[test]
fn why_unknown_member_fails() {
    let project = project();

    project
        .cmd()
        .args(["why", "vendor/alpha", "--member", "packages/nonexistent"])
        .assert()
        .failure();
}

#[test]
fn why_with_member_from_inside_member_dir() {
    let project = project();
    let member_dir = project.root().join("packages/api");

    // implicit scoping: invoked from inside member, no --member needed
    project
        .cmd()
        .args(["why", "vendor/alpha"])
        .current_dir(&member_dir)
        .assert()
        .success()
        .stdout(contains("acme/api"))
        .stdout(contains("root").not());
}

// ---------------------------------------------------------------------------
// phast outdated --member
// ---------------------------------------------------------------------------

#[test]
fn outdated_member_flag_is_accepted() {
    let project = project();

    let output = project
        .cmd()
        .args(["outdated", "--member", "packages/api"])
        .assert()
        .get_output()
        .clone();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unexpected argument"),
        "flag was rejected: {stderr}"
    );
}

#[test]
fn outdated_unknown_member_fails() {
    let project = project();

    project
        .cmd()
        .args(["outdated", "--member", "packages/nonexistent"])
        .assert()
        .failure();
}

#[test]
fn outdated_from_inside_member_scopes_implicitly() {
    let project = project();
    let member_dir = project.root().join("packages/api");

    let output = project
        .cmd()
        .arg("outdated")
        .current_dir(&member_dir)
        .assert()
        .get_output()
        .clone();

    // Must not error about missing lock or manifest; any other exit code is fine
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("phast.lock not found"),
        "lock resolution failed: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// phast audit (workspace-wide, no --member)
// ---------------------------------------------------------------------------

#[test]
fn audit_does_not_accept_member_flag() {
    let project = project();

    let output = project
        .cmd()
        .args([
            "audit",
            "--provider",
            "packagist",
            "--member",
            "packages/api",
        ])
        .assert()
        .failure()
        .get_output()
        .clone();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument") || stderr.contains("--member"),
        "expected rejection of --member on audit, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// phast run --member, against a member that inherits a constraint
// ---------------------------------------------------------------------------

#[tokio::test]
async fn run_member_script_when_the_member_inherits_a_constraint() {
    let project = inheriting_project();

    project
        .cmd()
        .args(["run", "check", "--member", "packages/api"])
        .assert()
        .success()
        .stdout(contains("MEMBER_SCRIPT"));
}

#[tokio::test]
async fn run_lists_the_scripts_of_an_inheriting_member() {
    let project = inheriting_project();

    project
        .cmd()
        .args(["run", "--member", "packages/api"])
        .assert()
        .success()
        .stdout(contains("check"));
}

#[tokio::test]
async fn run_from_inside_an_inheriting_member_needs_no_flag() {
    let project = inheriting_project();
    let member_dir = project.root().join("packages/api");

    project
        .cmd()
        .args(["run", "check"])
        .current_dir(&member_dir)
        .assert()
        .success()
        .stdout(contains("MEMBER_SCRIPT"));
}

#[tokio::test]
async fn run_at_the_workspace_root_runs_the_root_script() {
    let project = inheriting_project();

    project
        .cmd()
        .args(["run", "greet"])
        .assert()
        .success()
        .stdout(contains("ROOT_SCRIPT"));
}

// ---------------------------------------------------------------------------
// Lifecycle hooks in a workspace
// ---------------------------------------------------------------------------

#[test]
fn autoload_runs_a_hook_declared_at_the_workspace_root() {
    let project = inheriting_project();

    project
        .cmd()
        .arg("autoload")
        .assert()
        .success()
        .stdout(contains("ROOT_HOOK"));
}

#[test]
fn autoload_does_not_run_a_hook_declared_by_a_member() {
    let project = inheriting_project();

    project
        .cmd()
        .arg("autoload")
        .assert()
        .success()
        .stdout(contains("MEMBER_HOOK").not());
}

// ---------------------------------------------------------------------------
// Merge conflicts
// ---------------------------------------------------------------------------

#[test]
#[rustfmt::skip]
fn conflict_over_a_pinned_package_advises_inheriting() {
    let project = conflicting_project(
        indoc! {r#"
            [workspace.dependencies]
            "vendor/alpha" = "^1.0"
        "#}
    );

    project
        .cmd()
        .arg("workspace")
        .assert()
        .failure()
        .stderr(contains("{ workspace = true }"))
        .stderr(contains("pin a shared constraint").not());
}

#[test]
fn conflict_over_an_unpinned_package_advises_a_pin() {
    let project = conflicting_project("");

    project
        .cmd()
        .arg("workspace")
        .assert()
        .failure()
        .stderr(contains("pin a shared constraint"))
        .stderr(contains("{ workspace = true }").not());
}

// ---------------------------------------------------------------------------
// phast why attribution
// ---------------------------------------------------------------------------

#[test]
fn why_names_the_member_that_requires_a_package() {
    let project = attributing_project();

    project
        .cmd()
        .args(["why", "vendor/alpha"])
        .assert()
        .success()
        .stdout(contains("acme/api"))
        .stdout(contains("root").not());
}

#[test]
fn why_names_every_member_that_requires_a_package() {
    let project = attributing_project();

    project
        .cmd()
        .args(["why", "vendor/beta"])
        .assert()
        .success()
        .stdout(contains("acme/api"))
        .stdout(contains("acme/logger"));
}

#[test]
fn why_scoped_to_a_member_leaves_out_what_only_a_sibling_requires() {
    let project = attributing_project();

    // `--member` roots the graph at that member, so a package only its sibling requires is
    // outside it. Asserted on the attribution rather than the empty-case wording, which is
    // still phrased for a standalone project.
    project
        .cmd()
        .args(["why", "vendor/alpha", "--member", "packages/logger"])
        .assert()
        .success()
        .stdout(contains("acme/api").not());
}

#[test]
fn why_attributes_an_inherited_constraint_to_the_member() {
    let project = attributing_project();

    // acme/api takes vendor/alpha from [workspace.dependencies], so the member is still
    // the requirer and the constraint shown is the one it resolved to.
    project
        .cmd()
        .args(["why", "vendor/alpha"])
        .assert()
        .success()
        .stdout(contains("acme/api"))
        .stdout(contains("^1.0"));
}
