//! `vendor/bin/` executable stubs.
//!
//! Every locked package that declares `bin` entries gets a relative symlink in
//! `vendor/bin/` pointing at its executable. Unix only: on other platforms this is a
//! no-op, since the stubs would need to be batch files rather than symlinks.

use std::path::{Path, PathBuf};

use phast_core::lock::phast::PhastLock;

use anyhow::{Context, Result};

/// Manages `vendor/bin/` symlinks for installed packages.
pub struct Bin {
    /// `vendor/bin/`, where the stubs are written.
    bin_dir: PathBuf,
    /// The vendor root, used to resolve a stub's target and to tell our own
    /// symlinks from someone else's.
    vendor_dir: PathBuf,
}

#[rustfmt::skip]
impl Bin {
    /// Create a stub manager for the `vendor/` tree at `vendor_dir`.
    pub fn new(vendor_dir: &Path) -> Self {
        Self {
            bin_dir:    vendor_dir.join("bin"),
            vendor_dir: vendor_dir.to_owned(),
        }
    }

    /// Regenerate all stubs: remove stale ones, then create fresh ones from `lock`.
    #[cfg(unix)]
    pub fn generate(&self, lock: &PhastLock, include_dev: bool) -> Result<()> {
        // Clear stubs from a previous run (no-op if vendor/bin/ is absent).
        self.remove_stale_symlinks()?;

        let packages_dev: &[_] = if include_dev { &lock.packages_dev } else { &[] };

        let packages: Vec<_> = lock.packages
            .iter()
            .chain(packages_dev.iter())
            .filter(|package| !package.bin.is_empty())
            .collect();

        // No binaries to install: don't create (or keep) an empty vendor/bin/.
        if packages.is_empty() { return self.remove_empty_dir(); }

        std::fs::create_dir_all(&self.bin_dir)
            .context("failed to create vendor/bin")?;

        for package in packages {
            for bin in &package.bin { self.create_symlink(&package.name, bin)?; }
        }

        Ok(())
    }

    /// No-op off Unix, where a stub would have to be a batch file rather than a symlink.
    #[cfg(not(unix))]
    pub fn generate(&self, _lock: &PhastLock, _include_dev: bool) -> Result<()> {
        Ok(())
    }

    /// Create a relative symlink `vendor/bin/<file> -> ../<name>/<bin>` and set the
    /// executable bit on the target, where `<file>` is the file name of `bin`.
    ///
    /// Replaces whatever already sits at that path, a regular file included: a package
    /// that declares a binary owns that name in `vendor/bin/`.
    #[cfg(unix)]
    fn create_symlink(&self, name: &str, bin: &str) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let bin_name = Path::new(bin)
            .file_name()
            .context("bin entry has no file name")?;

        // Remove any existing stub so the symlink call below does not fail with EEXIST.
        let stub = self.bin_dir
            .join(bin_name);
        if stub.symlink_metadata().is_ok() {
            std::fs::remove_file(&stub)
                .with_context(|| format!("failed to remove existing stub {:?}", stub))?;
        }

        // Relative so vendor/ can be moved without breaking the link.
        let rel_target = Path::new("..")
            .join(name)
            .join(bin);
        std::os::unix::fs::symlink(&rel_target, &stub)
            .with_context(|| format!("failed to create symlink {:?} -> {:?}", stub, rel_target))?;

        // Some archives strip the executable bit; restore it so the binary is runnable.
        let target = self.vendor_dir
            .join(name)
            .join(bin);
        if target.exists() {
            let mut permissions = std::fs::metadata(&target)
                .with_context(|| format!("failed to read permissions of {:?}", target))?
                .permissions();
            permissions.set_mode(permissions.mode() | 0o111);
            std::fs::set_permissions(&target, permissions)
                .with_context(|| format!("failed to set executable bit on {:?}", target))?;
        }

        Ok(())
    }

    /// Remove `vendor/bin/` when it exists and is empty, so a project with no
    /// binaries (or whose last one was removed) is not left an empty directory.
    #[cfg(unix)]
    fn remove_empty_dir(&self) -> Result<()> {
        if !self.bin_dir.exists() {
            return Ok(());
        }

        let mut entries = std::fs::read_dir(&self.bin_dir)
            .context("failed to read vendor/bin")?;
        if entries.next().is_some() {
            return Ok(());
        }

        std::fs::remove_dir(&self.bin_dir)
            .context("failed to remove empty vendor/bin")?;
        Ok(())
    }

    /// Remove all symlinks in `vendor/bin/` that point into the vendor tree.
    ///
    /// Leaves regular files alone, so a script that no package claims survives. One
    /// whose name a package does declare is still replaced later, by `create_symlink`.
    #[cfg(unix)]
    fn remove_stale_symlinks(&self) -> Result<()> {
        if !self.bin_dir.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(&self.bin_dir)
            .context("failed to read vendor/bin")?;

        for entry in entries {
            let path = entry
                .context("failed to read vendor/bin entry")?
                .path();

            // Only touch symlinks; leave regular files (user scripts) alone.
            let is_symlink = path.symlink_metadata()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            if !is_symlink {
                continue;
            }

            // Skip entries whose target we cannot read (broken in an unexpected way).
            let Ok(target) = std::fs::read_link(&path) else {
                continue;
            };

            // Only remove stubs we created: relative targets or absolute paths inside vendor/.
            let is_managed = target.starts_with("..") || target.starts_with(&self.vendor_dir);
            if !is_managed {
                continue;
            }

            std::fs::remove_file(&path)
                .with_context(|| format!("failed to remove stale stub {:?}", path))?;
        }

        Ok(())
    }
}
