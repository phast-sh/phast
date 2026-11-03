//! The `phast cache` command surface.
//!
//! This is the one file whose subject is the cache rather than a project, so it hands
//! a seeded [`Cache`] to `Project::with_cache`. Everywhere else the cache is the
//! environment and stays invisible.
//!
//! Isolation matters more here than anywhere else. `cache clear` deletes whatever cache
//! it is pointed at, so a test that reached the real one would erase it.
//!
//! The cache is seeded directly rather than by installing something. `size` sums file
//! sizes and `clear` removes directories, so neither looks inside the files, and going
//! through a real install would only couple these tests to the download path.

mod common;

use common::cache::Cache;
use common::project::Project;

use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

/// A project whose cache holds one metadata entry and one archive.
fn project() -> Project {
    Project::with_cache(
        Cache::new()
            .write("metadata/vendor-alpha.json", "{\"packages\":{}}")
            .write("archives/vendor-alpha.zip", "not really a zip"),
    )
}

#[test]
fn cache_size_reports_each_area_and_a_total() {
    let project = project();

    project
        .cmd()
        .args(["cache", "size"])
        .assert()
        .success()
        .stdout(contains("metadata"))
        .stdout(contains("archives"))
        .stdout(contains("total"));
}

#[test]
fn cache_size_reports_zero_for_an_empty_cache() {
    let project = Project::new();

    project
        .cmd()
        .args(["cache", "size"])
        .assert()
        .success()
        .stdout(contains("total      0 B"));
}

#[test]
fn cache_size_metadata_only_reports_metadata_alone() {
    let project = project();

    project
        .cmd()
        .args(["cache", "size", "--metadata-only"])
        .assert()
        .success()
        .stdout(contains("metadata"))
        .stdout(contains("archives").not())
        .stdout(contains("total").not());
}

#[test]
fn cache_size_archives_only_reports_archives_alone() {
    let project = project();

    project
        .cmd()
        .args(["cache", "size", "--archives-only"])
        .assert()
        .success()
        .stdout(contains("archives"))
        .stdout(contains("metadata").not())
        .stdout(contains("total").not());
}

#[test]
fn cache_clear_removes_everything() {
    let project = project();

    project
        .cmd()
        .args(["cache", "clear"])
        .assert()
        .success()
        .stdout(contains("Cleared cache at"));

    assert!(!project.cache().join("metadata").exists());
    assert!(!project.cache().join("archives").exists());
}

#[test]
fn cache_clear_reports_the_space_it_freed() {
    let project = project();

    project
        .cmd()
        .args(["cache", "clear"])
        .assert()
        .success()
        .stdout(contains("(0 B freed)").not());
}

#[test]
fn cache_clear_metadata_only_keeps_the_archives() {
    let project = project();

    project
        .cmd()
        .args(["cache", "clear", "--metadata-only"])
        .assert()
        .success();

    assert!(!project.cache().join("metadata").exists());
    assert!(project.cache().join("archives").exists());
}

#[test]
fn cache_clear_archives_only_keeps_the_metadata() {
    let project = project();

    project
        .cmd()
        .args(["cache", "clear", "--archives-only"])
        .assert()
        .success();

    assert!(!project.cache().join("archives").exists());
    assert!(project.cache().join("metadata").exists());
}

/// The only test that overrides the cache directly: a [`Cache`] creates its directory,
/// so it cannot express one that is absent.
#[test]
fn cache_reports_a_missing_cache_rather_than_failing() {
    let project = Project::new();
    let missing = project.cache().join("no-such-cache");

    project
        .cmd()
        .env(common::CACHE_ENV, &missing)
        .args(["cache", "size"])
        .assert()
        .success()
        .stdout(contains("Cache directory does not exist"));
}

#[test]
fn cache_size_metadata_and_archives_flags_are_mutually_exclusive() {
    let project = Project::new();

    project
        .cmd()
        .args(["cache", "size", "--metadata-only", "--archives-only"])
        .assert()
        .failure();
}

#[test]
fn cache_clear_metadata_and_archives_flags_are_mutually_exclusive() {
    let project = Project::new();

    project
        .cmd()
        .args(["cache", "clear", "--metadata-only", "--archives-only"])
        .assert()
        .failure();
}
