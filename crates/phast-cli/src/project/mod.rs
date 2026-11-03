//! Locating and reading the project a command operates on.
//!
//! Three things a command settles before it can do any work: which directory is the project
//! ([`workdir`]), what its lock file holds ([`lock`]), and which manifest sections it may
//! edit ([`manifest`]).

pub mod lock;
pub mod manifest;

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Resolve the working directory from an optional `path`.
///
/// Falls back to the current directory when the path is absent. A path that was given comes
/// back exactly as it arrived, neither made absolute nor checked for existence, so a relative
/// `--project-dir` stays relative to the shell's directory and a mistyped one is reported
/// further downstream as "phast.toml not found" rather than as the missing directory it is.
#[rustfmt::skip]
pub fn workdir(path: Option<PathBuf>) -> Result<PathBuf> {
    path.map_or_else(|| {
        std::env::current_dir()
            .context("failed to get current directory")
    }, Ok)
}
