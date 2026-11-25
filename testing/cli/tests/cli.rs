//! The `phast cli` command surface.
//!
//! `cli version` is pure and offline, so it is covered directly. `cli update` is never
//! invoked here: it queries api.github.com for the latest release and then replaces the
//! running executable, which is not something a test should do. Only its argument
//! surface is checked, through `--help`, which clap answers before the command runs.

mod common;

use common::project::Project;

use predicates::function::function;
use predicates::str::contains;

/// `cli` reads no project files, so an empty directory is enough.
fn project() -> Project {
    Project::new()
}

#[test]
fn cli_version_names_the_binary_and_its_version() {
    let project = project();

    let output = project
        .cmd()
        .args(["cli", "version"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    let first = stdout.lines().next().expect("no output");

    assert!(
        first.starts_with("phast "),
        "expected a `phast <version> <release>` line, got '{first}'"
    );
    assert!(
        first.split_whitespace().count() >= 3,
        "expected a release name after the version, got '{first}'"
    );
}

/// The build stamp comes from `build.rs`, so its absence means the binary was built
/// without the metadata this command exists to report.
#[test]
fn cli_version_reports_the_build_time() {
    let project = project();

    project
        .cmd()
        .args(["cli", "version"])
        .assert()
        .success()
        .stdout(contains("build:"));
}

/// `unknown` is the sentinel `build.rs` emits for a field it could not determine.
#[test]
fn cli_version_omits_fields_it_could_not_determine() {
    let project = project();

    project
        .cmd()
        .args(["cli", "version"])
        .assert()
        .success()
        .stdout(function(|stdout: &str| {
            stdout
                .lines()
                .all(|line| line.split_whitespace().last() != Some("unknown"))
        }));
}

#[test]
fn cli_version_prints_nothing_when_quiet() {
    let project = project();

    project
        .cmd()
        .args(["cli", "version", "--quiet"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn cli_requires_a_subcommand() {
    let project = project();

    project
        .cmd()
        .arg("cli")
        .assert()
        .failure()
        .stderr(contains("requires a subcommand"))
        .stderr(contains("update"))
        .stderr(contains("version"));
}

#[test]
fn cli_rejects_an_unknown_subcommand() {
    let project = project();

    project
        .cmd()
        .args(["cli", "nonsense"])
        .assert()
        .failure()
        .stderr(contains("unrecognized subcommand 'nonsense'"));
}

/// Covers that `update` exists and parses, without running it. `--help` is answered by
/// clap, so nothing reaches GitHub and the executable is left alone.
#[test]
fn cli_update_accepts_help_without_running() {
    let project = project();

    project
        .cmd()
        .args(["cli", "update", "--help"])
        .assert()
        .success()
        .stdout(contains("Download and apply the latest Phast release"));
}
