//! Installing local `path` repository packages into `vendor/`.
//!
//! The counterpart to [`Downloader`](phast_registry::http::downloader::Downloader)
//! for packages that live in a local directory instead of a registry archive.
//! Such a package is materialized by symlinking `vendor/<name>` at its source
//! tree, so edits to the source are visible without a reinstall.
//!
//! Only on Unix. Elsewhere [`link`](PathLinker::link) is compiled as a no-op, so a `path`
//! package resolves and locks like any other and then never reaches `vendor/`, with nothing
//! said about it.
//!
//! The entry being replaced is assumed to be a symlink or absent. A real directory there,
//! left by an earlier install from an archive, is removed with `remove_file` and fails:
//! `failed to remove existing vendor/acme/logger`. `install` hides this by skipping any
//! package whose directory is already populated, so it surfaces through `update`, which
//! relinks a package whose version moved whatever is in the way.

use std::fs as StdFs;
use std::os::unix::fs as UnixFs;
use std::path::{Path, PathBuf};

use phast_core::lock::phast::PackageDef;

use anyhow::{Context, Result};

/// Symlinks local `path` packages into a `vendor/` directory.
pub struct PathLinker {
    /// Absolute path to the `vendor/` directory packages are linked into.
    vendor_dir: PathBuf,
}

#[rustfmt::skip]
impl PathLinker {
    pub fn new(vendor_dir: &Path) -> Self {
        Self {
            vendor_dir: vendor_dir.to_owned(),
        }
    }

    /// Symlink each package into `vendor/<name>`.
    ///
    /// Unix only; a no-op on other platforms until copy-based materialization
    /// lands.
    #[cfg(unix)]
    pub fn link(&self, packages: &[PackageDef]) -> Result<()> {
        for package in packages { self.link_one(package)?; }

        Ok(())
    }

    #[cfg(not(unix))]
    pub fn link(&self, _packages: &[PackageDef]) -> Result<()> {
        Ok(())
    }

    /// Create a relative symlink `vendor/<name>` pointing at the package's local
    /// source directory.
    ///
    /// The target is relative so `vendor/` stays movable. A relative target resolves against
    /// the directory holding the link, and that is always `vendor/<vendor>/`, two below the
    /// project root, because a package name is always two segments. So `../..` lands back at
    /// the root, where the recorded source path is anchored: `acme/logger` becomes
    /// `vendor/acme/logger -> ../../libs/logger`.
    #[cfg(unix)]
    fn link_one(&self, package: &PackageDef) -> Result<()> {
        let source = package
            .dist
            .as_ref()
            .map(|dist| dist.url.as_str())
            .context("path package has no source directory")?;

        let link = self.vendor_dir.join(&package.name);
        if let Some(parent) = link.parent() {
            StdFs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        // Replace any existing entry so the symlink call does not fail with EEXIST.
        if link.symlink_metadata().is_ok() {
            StdFs::remove_file(&link)
                .with_context(|| format!("failed to remove existing {}", link.display()))?;
        }

        let target = Path::new("../..").join(source);
        UnixFs::symlink(&target, &link)
            .with_context(|| format!("failed to symlink {} -> {}", link.display(), target.display()))?;

        Ok(())
    }
}
