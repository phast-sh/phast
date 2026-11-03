//! The `phast audit` command surface.
//!
//! Everything here is offline. `audit` reads the lock, and returns before opening a
//! connection when there is nothing to ask about, so the argument surface and both
//! early exits are reachable without a network.
//!
//! What is deliberately not covered is a run that reaches an advisory service.
//! `PackagistAdvisoryProvider` and `OsvAdvisoryProvider` hold their hosts as constants
//! with no override, and adding one so tests could redirect them would be a seam worth
//! not having. Advisory matching belongs in library-tier tests against the providers.

mod common;

use common::project::Project;

use indoc::indoc;
use predicates::str::contains;

const MANIFEST: &str = indoc! {r#"
    [package]
    name = "acme/blog"
"#};

/// A lock recording no packages at all.
const EMPTY_LOCK: &str = indoc! {r#"
    version      = 1
    content-hash = "sha256:test"

    [metadata]
    generated-by = "phast 0.1.0"
    generated-at = "2026-01-01T00:00:00+00:00"
"#};

/// A project with a lock, but nothing in it to audit.
fn project() -> Project {
    Project::new().manifest(MANIFEST).lock(EMPTY_LOCK)
}

/// With nothing installed there is nothing to ask about, so `audit` says so and stops
/// rather than querying a service for an empty list.
#[test]
fn audit_reports_when_nothing_is_installed() {
    let project = project();

    project
        .cmd()
        .args(["audit", "--provider", "packagist"])
        .assert()
        .success()
        .stdout(contains("No packages installed."));
}

#[test]
fn audit_accepts_either_provider() {
    for provider in ["packagist", "osv"] {
        let project = project();

        project
            .cmd()
            .args(["audit", "--provider", provider])
            .assert()
            .success()
            .stdout(contains("No packages installed."));
    }
}

#[test]
fn audit_accepts_a_project_dir() {
    let project = project();
    let elsewhere = Project::new();

    elsewhere
        .cmd()
        .args(["audit", "--provider", "osv", "--project-dir"])
        .arg(project.root())
        .assert()
        .success()
        .stdout(contains("No packages installed."));
}

#[test]
fn audit_accepts_its_filter_flags() {
    let project = project();

    project
        .cmd()
        .args(["audit", "--provider", "osv"])
        .args(["--no-dev", "--since", "2026-01-01"])
        .args(["--ignore", "CVE-2023-24813", "--ignore", "CVE-2024-0001"])
        .assert()
        .success();
}

#[test]
fn audit_accepts_a_json_format() {
    let project = project();

    project
        .cmd()
        .args(["audit", "--provider", "osv", "--format", "json"])
        .assert()
        .success();
}

/// The lock is read before anything is queried, so a project that was never installed
/// fails locally rather than after a round trip.
#[test]
fn audit_fails_without_a_lock() {
    let project = Project::new().manifest(MANIFEST);

    project
        .cmd()
        .args(["audit", "--provider", "packagist"])
        .assert()
        .failure()
        .stderr(contains("phast.lock not found"));
}

/// A bare `phast audit` runs. Which database it picked is asserted through `--help`,
/// because the choice is only otherwise visible in the request this tier does not make.
#[test]
fn audit_defaults_to_the_packagist_provider() {
    let project = project();

    project
        .cmd()
        .arg("audit")
        .assert()
        .success()
        .stdout(contains("No packages installed."));

    project
        .cmd()
        .args(["audit", "--help"])
        .assert()
        .success()
        .stdout(contains("[default: packagist]"));
}

#[test]
fn audit_rejects_an_unknown_provider() {
    let project = project();

    project
        .cmd()
        .args(["audit", "--provider", "nonsense"])
        .assert()
        .failure()
        .stderr(contains("invalid value 'nonsense'"))
        .stderr(contains("packagist"))
        .stderr(contains("osv"));
}

#[test]
fn audit_rejects_a_malformed_since_date() {
    let project = project();

    project
        .cmd()
        .args(["audit", "--provider", "osv", "--since", "notadate"])
        .assert()
        .failure()
        .stderr(contains("--since"));
}
