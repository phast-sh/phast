mod common;

use common::project::Project;

use indoc::indoc;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::{contains, is_match};

/// A project with two scripts: a single command, and a list of two.
const MANIFEST: &str = indoc! {r#"
    [package]
    name = "acme/my-project"
    type = "project"

    [scripts]
    greet = "echo hello from phast"
    multi = ["echo step1", "echo step2"]
"#};

/// A project whose scripts call each other with `@`, including a cycle
/// and a call to a script that doesn't exist.
const MANIFEST_WITH_REFERENCES: &str = indoc! {r#"
    [package]
    name = "acme/my-project"
    type = "project"

    [scripts]
    lint   = "echo lint"
    test   = ["@lint", "echo test"]
    ping   = "@pong"
    pong   = "@ping"
    broken = ["echo ran", "@missing"]
"#};

/// A project declaring two scripts.
fn project() -> Project {
    Project::new().manifest(MANIFEST)
}

#[tokio::test]
async fn run_executes_shell_script() {
    let project = project();

    project
        .cmd()
        .args(["run", "greet"])
        .assert()
        .success()
        .stdout(contains("> echo hello from phast"));
}

#[tokio::test]
async fn run_executes_all_steps_in_multi_script() {
    let project = project();

    project
        .cmd()
        .args(["run", "multi"])
        .assert()
        .success()
        .stdout(contains("> echo step1"))
        .stdout(contains("> echo step2"));
}

#[tokio::test]
async fn run_appends_extra_args_to_last_step() {
    let project = project();

    project
        .cmd()
        .args(["run", "greet", "--", "--verbose"])
        .assert()
        .success()
        .stdout(contains("> echo hello from phast --verbose"));
}

#[tokio::test]
async fn run_fails_on_unknown_script() {
    let project = project();

    project
        .cmd()
        .args(["run", "no-such-script"])
        .assert()
        .failure()
        .stderr(contains("no-such-script"));
}

#[tokio::test]
async fn run_fails_without_manifest() {
    let project = Project::new();

    project.cmd().args(["run", "greet"]).assert().failure();
}

// ---------------------------------------------------------------------------
// Scripts that call other scripts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn run_executes_a_referenced_script() {
    let project = Project::new().manifest(MANIFEST_WITH_REFERENCES);

    project
        .cmd()
        .args(["run", "test"])
        .assert()
        .success()
        .stdout(is_match("(?s)> echo lint.*> echo test").unwrap());
}

#[tokio::test]
async fn run_fails_on_a_cyclic_reference() {
    let project = Project::new().manifest(MANIFEST_WITH_REFERENCES);

    project
        .cmd()
        .args(["run", "ping"])
        .assert()
        .failure()
        .stderr(contains("ping → pong → ping"));
}

#[tokio::test]
async fn run_fails_on_a_missing_reference_before_running_anything() {
    let project = Project::new().manifest(MANIFEST_WITH_REFERENCES);

    project
        .cmd()
        .args(["run", "broken"])
        .assert()
        .failure()
        .stderr(contains("missing"))
        .stdout(contains("> echo ran").not());
}
