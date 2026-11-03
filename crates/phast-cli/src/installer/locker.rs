//! Reading, staleness-checking, and writing `phast.lock`.
//!
//! [`Locker`] owns the lock path and the progress sink; everything it needs from
//! the project (manifest fingerprint, member entries, autoload rules) is borrowed
//! from the [`Workspace`] per call. The Installer hands it a resolution plus the
//! provider that produced it, and gets back a written lock.

use super::resolver::Resolved;
use crate::output::Printer;

use std::path::PathBuf;
use std::time::Instant;

use phast_core::lock::hash;
use phast_core::lock::phast::{AutoloadSection, PhastLock, PhastLockState};
use phast_workspace::Workspace;

use anyhow::{Context, Result};

/// Manages the `phast.lock` file: staleness fingerprint, load/classify, write.
pub struct Locker {
    /// Absolute path to `phast.lock` at the workspace root.
    lock_path: PathBuf,
    /// Progress and status output sink.
    printer: Printer,
}

impl Locker {
    pub fn new(lock_path: PathBuf, printer: Printer) -> Self {
        Self { lock_path, printer }
    }

    /// Load `phast.lock` and classify it against `content_hash`.
    ///
    /// Returns `Fresh` when it exists and is up to date, `Stale` when it exists but the hash
    /// has changed, and `Missing` when absent or unreadable. Those last two are one variant
    /// on purpose: a lock that will not parse is discarded and re-resolved rather than
    /// raised, so a corrupt file costs a warning and a solve, not a failed command.
    ///
    /// All three outcomes are announced from here rather than by the caller, which is why
    /// `Installer::lock` can return early on `Fresh` and still not be silent.
    pub fn load(&self, content_hash: &str) -> PhastLockState {
        if !self.lock_path.exists() {
            return PhastLockState::Missing;
        }

        match PhastLock::read(&self.lock_path) {
            Ok(lock) if lock.is_fresh(content_hash) => {
                self.printer.info("Lock file is up to date.");
                PhastLockState::Fresh(lock)
            }
            Ok(lock) => {
                self.printer
                    .warning("Lock file is out of date, re-resolving.");
                PhastLockState::Stale(lock)
            }
            Err(e) => {
                self.printer
                    .warning(format!("Could not read phast.lock ({}), re-resolving.", e));
                PhastLockState::Missing
            }
        }
    }

    /// Read the raw lock if present, ignoring staleness.
    ///
    /// Three callers want the prior lock whatever its hash says, for two different reasons.
    /// `add` and `lock --refresh` hand it to the solver as preferences, so a package nobody
    /// asked about keeps the version it had. `update` never does that, it passes the solver
    /// no lock at all, and reads this one only to pin the packages it was not asked to move
    /// and to work out afterward which versions changed.
    pub fn read(&self) -> Option<PhastLock> {
        PhastLock::read(&self.lock_path).ok()
    }

    /// Enrich a resolution into a lock, append workspace members, and write it.
    ///
    /// Enrichment reads the provider's metadata cache rather than the network, which is
    /// why [`Resolved`] carries the provider that answered the solve.
    #[rustfmt::skip]
    pub async fn write(
        &self,
        resolved: Resolved,
        content_hash: &str,
        workspace: &Workspace,
    ) -> Result<PhastLock> {
        let start = Instant::now();
        let Resolved { resolution, provider } = resolved;

        let spinner = self.printer.spinner("Locking...");
        let mut lock = provider
            .enrich(
                &resolution,
                content_hash,
                AutoloadSection::from(workspace.manifest().autoload.clone()),
                AutoloadSection::from(workspace.manifest().autoload_dev.clone()),
                |name, version| {
                    spinner.tick("Locking", name, version);
                },
            )
            .await
            .context("failed to enrich resolution with package metadata")?;
        spinner.finish_and_clear();

        // Workspace members are recorded as package entries marked by `path`.
        // Appending keeps the topological order: members depend on external
        // packages and nothing in the lock depends on a member.
        lock.packages.extend(workspace.lock());
        lock.write(&self.lock_path)
            .context("failed to write phast.lock")?;

        let total = lock.packages.len() + lock.packages_dev.len();
        self.printer.phase("Locked", total, start.elapsed().as_secs_f64());

        Ok(lock)
    }

    /// Staleness fingerprint over the manifest and workspace member entries.
    ///
    /// Recomputed on every command and compared against the stored hash to
    /// decide whether the lock can be reused without re-resolving.
    pub fn content_hash(&self, workspace: &Workspace) -> Result<String> {
        hash::compute(workspace.manifest(), &workspace.lock())
            .context("failed to compute lock content hash")
    }
}
