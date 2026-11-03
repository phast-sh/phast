mod common;

use common::project::Project;

use indoc::indoc;
use predicates::str::contains;

const MANIFEST: &str = indoc! {r#"
    [package]
    name = "acme/my-project"
    type = "project"

    [scripts]
    greet = "echo hello from phast"
    multi = ["echo step1", "echo step2"]
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
