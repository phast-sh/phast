//! Classmap exclusion: path patterns a package keeps out of its classmap.
//!
//! A package (the root project, a dependency, or a workspace member) may declare
//! path patterns that are skipped while building the classmap. They arrive from
//! the package's autoload metadata or from `[autoload] exclude` in a native
//! manifest, and land here as one list.
//!
//! Semantics, the same for both sources:
//!
//! - patterns are relative to the declaring package's root;
//! - a leading `/` anchors the pattern at that root, otherwise it may match at any depth in the package tree;
//! - a trailing `/` names a directory, so everything under it is excluded.
//! - `*` matches within a path segment.
//! - `**` matches across segments (via [`globset`]);
//!
//! A matcher only ever tests files inside its own package root; a file elsewhere
//! is never excluded.

use std::path::{Path, PathBuf};

use anyhow::Result;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

/// A compiled classmap-exclusion matcher for one package tree.
#[derive(Debug)]
pub struct Exclude {
    /// The package root that patterns are resolved against.
    root: PathBuf,
    /// The compiled patterns, tested against package-root-relative paths.
    set: GlobSet,
}

impl Exclude {
    /// Compile `patterns` (relative to `root`) into a matcher, or `None` when there
    /// is nothing to exclude.
    ///
    /// Returns an error if a pattern is not a valid glob.
    pub fn new(root: &Path, patterns: &[String]) -> Result<Option<Self>> {
        if patterns.is_empty() {
            return Ok(None);
        }

        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            for glob in globs_for(pattern) {
                // `literal_separator` keeps `*` inside one path segment. Without it a
                // pattern like `src/Legacy/*` also swallows `src/Legacy/deep/Old.php`,
                // and a class excluded by accident is a hard failure under an
                // authoritative classmap, where there is no filesystem fallback.
                builder.add(GlobBuilder::new(&glob).literal_separator(true).build()?);
            }
        }

        Ok(Some(Self {
            root: root.to_path_buf(),
            set: builder.build()?,
        }))
    }

    /// Whether this matcher excludes `file`.
    ///
    /// The file is made relative to the package root before matching;
    /// a file outside the root is never excluded.
    pub fn excludes(&self, file: &Path) -> bool {
        file.strip_prefix(&self.root)
            .is_ok_and(|rel| self.set.is_match(rel))
    }
}

/// Translate one exclude pattern into the globs that implement it.
///
/// An anchored pattern (leading `/`) yields one glob rooted at the package
/// root. A floating pattern yields two: the same glob, plus a `**/`-prefixed
/// variant so it also matches nested occurrences.
///
/// Examples (pattern -> globs):
///
/// - `/Test/`        -> `["Test/**"]`
/// - `Test/`         -> `["**/Test/**", "Test/**"]`
/// - `src/Legacy/*`  -> `["**/src/Legacy/*", "src/Legacy/*"]`
/// - `/config.php`   -> `["config.php"]`
fn globs_for(pattern: &str) -> Vec<String> {
    let normalized = pattern.replace('\\', "/");
    let anchored = normalized.starts_with('/');

    let mut glob = normalized.trim_start_matches('/').to_string();
    // A trailing `/` names a directory: exclude everything beneath it.
    if glob.ends_with('/') {
        glob.push_str("**");
    }

    if glob.is_empty() {
        return Vec::new();
    }

    if anchored {
        // Anchored: match only at the package root, exactly as written.
        vec![glob]
    } else {
        // Floating: match at the root, and at any nested depth via the
        // `**/` prefix (so `Test/` also catches `src/Test/`).
        vec![format!("**/{glob}"), glob]
    }
}
