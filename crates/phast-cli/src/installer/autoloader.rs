//! Generating `vendor/autoload.php` and `vendor/bin/` from a written lock.
//!
//! [`Autoloader`] owns the `vendor/` path, the progress sink, and the lifecycle manager
//! scoped to its autoload-dump events. [`generate`](Autoloader::generate) is the entry point,
//! and the hooks it fires wrap only the dump: `vendor/bin/` is linked after
//! `PostAutoloadDump` has already run, so a script bound to that event runs before the stubs
//! are relinked. The workspace, whose member source trees are registered in place rather than
//! copied into `vendor/`, is borrowed per call.

use crate::output::Printer;

use std::path::PathBuf;
use std::time::Instant;

use phast_autoload::AutoloadOptions;
use phast_core::lock::phast::PhastLock;
use phast_runtime::event::{LifecycleEvent, LifecycleEventManager};
use phast_workspace::Workspace;

use anyhow::{Context, Result};

/// Generates the autoloader and bin stubs under `vendor/`.
pub struct Autoloader {
    /// Absolute path to the `vendor/` directory at the workspace root.
    vendor_dir: PathBuf,
    /// Handlers for the autoload-dump lifecycle events this loader fires.
    lifecycle: LifecycleEventManager,
    /// Progress and status output sink.
    printer: Printer,
}

impl Autoloader {
    /// Build a loader for one `vendor/` directory.
    ///
    /// `vendor_dir` is that directory, not the workspace root holding it. Both are `PathBuf`,
    /// so handing over the root type-checks and writes `autoload.php` a level too high.
    pub fn new(vendor_dir: PathBuf, lifecycle: LifecycleEventManager, printer: Printer) -> Self {
        Self {
            vendor_dir,
            lifecycle,
            printer,
        }
    }

    /// Generate the autoloader within its dump hooks, then link bin stubs.
    ///
    /// Fires `PreAutoloadDump`, generates `vendor/autoload.php` and its maps,
    /// fires `PostAutoloadDump`, then links `vendor/bin/`. Workspace member
    /// source trees are registered in place via `workspace`, so they are
    /// autoloadable without being copied into `vendor/`.
    pub fn generate(
        &self,
        lock: &PhastLock,
        options: AutoloadOptions,
        workspace: &Workspace,
    ) -> Result<()> {
        self.lifecycle.fire(LifecycleEvent::PreAutoloadDump)?;
        self.dump(lock, options, workspace)?;
        self.lifecycle.fire(LifecycleEvent::PostAutoloadDump)?;
        self.link_bins(lock, !options.include_dev)
    }

    /// Generate `vendor/autoload.php` and classmaps from the lock.
    fn dump(
        &self,
        lock: &PhastLock,
        options: AutoloadOptions,
        workspace: &Workspace,
    ) -> Result<()> {
        let start = Instant::now();

        let report = phast_autoload::generate(lock, &self.vendor_dir, options, workspace)
            .context("failed to generate autoloader")?;

        for duplicate in &report.duplicates {
            self.printer.warning(format!(
                "Duplicate class '{}': using {}, ignoring {}",
                duplicate.class, duplicate.kept, duplicate.ignored,
            ));
        }

        let total = lock.packages.len() + lock.packages_dev.len();
        self.printer
            .phase("Autoloaded", total, start.elapsed().as_secs_f64());

        Ok(())
    }

    /// Create `vendor/bin/` symlinks for every package that declares a `bin` entry.
    fn link_bins(&self, lock: &PhastLock, no_dev: bool) -> Result<()> {
        phast_autoload::bin::Bin::new(&self.vendor_dir)
            .generate(lock, !no_dev)
            .context("failed to generate vendor/bin stubs")
    }
}
