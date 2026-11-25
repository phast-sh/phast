//! Emptying `vendor/` of what left the lock.
//!
//! What leaves is decided from what the last install recorded, never from what happens to be
//! on disk, so a directory Phast did not place is never touched.

use crate::output::Printer;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use phast_autoload::installed;
use phast_core::PackageName;
use phast_core::lock::phast::PackageDef;

use anyhow::{Context, Result};

/// Empties `vendor/` of what the last install placed and the lock no longer keeps.
pub struct Pruner {
    /// The `vendor/` directory packages are removed from.
    vendor_dir: PathBuf,
    /// Progress and status output sink.
    printer: Printer,
}

impl Pruner {
    /// Build a pruner for one `vendor/` directory.
    pub fn new(vendor_dir: PathBuf, printer: Printer) -> Self {
        Self {
            vendor_dir,
            printer,
        }
    }

    /// Keep `kept` packages in `vendor/` and remove whatever else the last install recorded.
    ///
    /// Run this before autoloading. The old `installed.json` says what to remove, and
    /// autoloading replaces it.
    pub fn retain(&self, kept: &[PackageDef]) -> Result<()> {
        let start = Instant::now();

        let written = installed::written(&self.vendor_dir);
        let removed = remove(&self.vendor_dir, &stale(&written, kept))?;

        if removed > 0 {
            self.printer
                .phase("Removed", removed, start.elapsed().as_secs_f64());
        }

        Ok(())
    }
}

/// The names in `written` that `kept` no longer holds, and so leave `vendor/`.
#[rustfmt::skip]
pub fn stale(written: &[PackageName], kept: &[PackageDef]) -> Vec<PackageName> {
    let kept: HashSet<&str> = kept
        .iter()
        .map(|p| p.name.as_str())
        .collect();

    written
        .iter()
        .filter(|name| !kept.contains(name.as_str()))
        .cloned()
        .collect()
}

/// Remove `names` from `vendor_dir`, then each vendor-name directory left empty, returning
/// how many were removed.
#[rustfmt::skip]
pub fn remove(vendor_dir: &Path, names: &[PackageName]) -> Result<usize> {
    let mut removed = 0;

    for name in names {
        // The vendor half of `vendor/package` is the directory the package sits in.
        let Some((vendor_name, _)) = name.as_str().split_once('/') else {
            continue
        };

        // A symlinked vendor-name directory could point anywhere, so only a real one is entered.
        let parent = vendor_dir.join(vendor_name);
        if !parent.symlink_metadata().is_ok_and(|meta| meta.is_dir()) {
            continue;
        }

        // `symlink_metadata` doesn't follow links, so a path package shows up as the link it is.
        let path = vendor_dir.join(name.as_str());
        let Ok(meta) = path.symlink_metadata() else {
            continue
        };

        // A path package links to its source, so only the link is removed; others are deleted.
        let result = if meta.is_symlink() {
            fs::remove_file(&path)
        } else if meta.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            continue;
        };
        result.with_context(|| format!("failed to remove {}", path.display()))?;
        removed += 1;

        // `remove_dir` refuses a directory that isn't empty, so this only drops the vendor-name
        // directory once its last package is gone.
        let _ = fs::remove_dir(&parent);
    }

    Ok(removed)
}
