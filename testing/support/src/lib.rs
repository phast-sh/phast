//! Shared harness for the crates under `testing/`.
//!
//! Three tiers exercise the shipped binary and all three need the same primitives:
//! a [`Command`] pointing at that binary, a scratch directory that cleans itself up,
//! and (for the hermetic tier) a mock registry to resolve against.
//!
//! - [`phast_cmd`] and [`temp_dir`] are the primitives every tier uses. Every
//!   command runs with an isolated cache, so no test can reach the real one.
//! - [`hints`] holds the failure message for every workflow assertion.
//! - [`project`] stages a temporary project directory on disk.
//! - [`cache`] stages a cache directory, for tests whose subject is the cache.
//! - [`registry`] builds the wiremock endpoints a hermetic install needs.
//! - [`workflow`] parses `workflow.toml` specs and runs them.
//!
//! The binary under test is resolved once by [`phast_bin`]: `PHAST_BIN` when set,
//! otherwise Cargo's own build of `phast`. Tiers that run against a prebuilt binary
//! set the variable rather than paying for a rebuild.

pub(crate) mod fs;

pub mod cache;
pub mod hints;
pub mod project;
pub mod registry;
pub mod workflow;

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

/// Environment variable naming a prebuilt `phast` binary to test against.
pub const BIN_ENV: &str = "PHAST_BIN";

/// Environment variable the binary reads for its cache location.
pub const CACHE_ENV: &str = "PHAST_CACHE_DIR";

/// Path to the `phast` binary under test.
///
/// Prefers `PHAST_BIN` so a CI job can build once and share the artifact
/// across tiers. Falls back to the binary Cargo built for this test run.
pub fn phast_bin() -> PathBuf {
    match std::env::var_os(BIN_ENV) {
        Some(path) => PathBuf::from(path),
        None => assert_cmd::cargo::cargo_bin("phast"),
    }
}

/// Return a [`Command`] pointing at the `phast` binary under test.
/// The binary under test, run in `workdir` with `cache_dir` as its cache.
///
/// Both are required rather than defaulted. Left to itself the binary uses the
/// developer's real cache, so tests would fill it with mock archives, read each
/// other's entries through it, and `phast cache clear` would delete it outright.
pub fn phast_cmd(workdir: &Path, cache_dir: &Path) -> Command {
    let bin = phast_bin();
    assert!(bin.exists(), "phast binary not found at {}", bin.display());

    let mut command = Command::new(bin);
    command.current_dir(workdir).env(CACHE_ENV, cache_dir);
    command
}

/// Create a temporary directory that is deleted when the returned handle drops.
pub fn temp_dir() -> TempDir {
    TempDir::new().expect("failed to create temp dir")
}
