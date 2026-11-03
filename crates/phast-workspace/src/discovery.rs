//! Finding a workspace on disk.
//!
//! [`find_root`] walks upward for the `phast.toml` that declares `[workspace]`, and
//! `MemberPatterns` expands that file's globs into the directories that actually hold
//! a member manifest. Reading those manifests is `Workspace::load`'s job.
//!
//! Both halves are quiet about what they reject. A `phast.toml` that does not parse is
//! passed over as though the file were not there, and a directory matching a member
//! glob without a manifest is simply not a member. A malformed member surfaces later,
//! when `Workspace::load` reads it; a malformed root manifest never surfaces at all,
//! because the walk continues past it and the project looks standalone.

use std::fs;
use std::fs::read_to_string;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use phast_core::manifest::phast::PhastManifest;

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};

/// Glob patterns declared in `[workspace].members` and `[workspace].exclude`.
///
/// Holds them as written, so the manifest stays the record of what the user asked
/// for. [`MemberPatterns::expand`] is what turns them into directories on disk.
pub(crate) struct MemberPatterns {
    /// Raw glob patterns from `[workspace].members`, relative to the workspace root.
    pub(crate) patterns: Vec<String>,
    /// Raw glob patterns from `[workspace].exclude`, relative to the workspace root.
    pub(crate) exclude: Vec<String>,
}

impl MemberPatterns {
    /// Expand the member globs against `root`, returning the matching directories
    /// that hold a `phast.toml`, sorted by path.
    ///
    /// A match ends the descent. Once a directory is taken as a member nothing below
    /// it is looked at, so a member nested inside another member is never found. An
    /// exclusion ends the descent too, which is what makes `exclude = ["vendor"]`
    /// cover everything under `vendor/` rather than that one directory.
    ///
    /// A pattern that matches nothing is not an error here. `Workspace::load` is what
    /// refuses a workspace whose globs turned up no members.
    #[rustfmt::skip]
    pub(crate) fn expand(&self, root: &Path) -> Result<Vec<PathBuf>> {
        let mut builder = GlobSetBuilder::new();
        for pattern in &self.patterns {
            let glob = Glob::new(pattern)
                .with_context(|| format!("invalid workspace member pattern: {pattern}"))?;
            builder.add(glob);
        }
        let members = builder
            .build()
            .context("failed to build workspace glob set")?;

        let mut builder = GlobSetBuilder::new();
        for pattern in &self.exclude {
            let glob = Glob::new(pattern)
                .with_context(|| format!("invalid workspace exclude pattern: {pattern}"))?;
            builder.add(glob);
        }
        let exclude = builder
            .build()
            .context("failed to build workspace exclude glob set")?;

        let mut results = Vec::new();
        // A pattern can match at any depth, so every directory below the root is
        // visited and tested rather than the patterns being used to build paths.
        walk_dirs(root, root, &members, &exclude, &mut results);
        results.sort();

        Ok(results)
    }
}

/// Walk upward from `start` looking for a `phast.toml` that declares `[workspace]`.
///
/// Returns the workspace root path when found, or `None` once the walk runs out
/// of parents. A `phast.toml` that fails to parse counts as no match rather than
/// as an error, so a typo in a workspace root leaves the walk to continue past it.
///
/// `start` must be absolute. A relative path runs out of parents at the working
/// directory, and the walk never reaches above it.
pub fn find_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if read_to_string(dir.join("phast.toml"))
            .ok()
            .and_then(|c| PhastManifest::from_str(&c).ok())
            .is_some_and(|m| m.workspace.is_some())
        {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Recursively walk `dir`, collecting subdirectories that match `members` and hold
/// a `phast.toml`. Matching is done on the path relative to `root`, which is why
/// the workspace root is carried through the recursion rather than derived from `dir`.
/// A directory matched by `exclude` is skipped even if it also matches `members`.
fn walk_dirs(
    root: &Path,
    dir: &Path,
    members: &GlobSet,
    exclude: &GlobSet,
    results: &mut Vec<PathBuf>,
) {
    // Skip directories we cannot read (e.g. permission errors).
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        // Only directories can be workspace members.
        if !path.is_dir() {
            continue;
        }

        // Glob patterns are relative to the workspace root, so we need the relative path.
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };

        // Excluded directories are skipped entirely, including their subtree.
        if exclude.is_match(rel) {
            continue;
        }

        // A directory is a member when it matches a declared pattern and contains a manifest.
        if members.is_match(rel) && path.join("phast.toml").exists() {
            results.push(path.clone());
            continue;
        }

        walk_dirs(root, &path, members, exclude, results);
    }
}
