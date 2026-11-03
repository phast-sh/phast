//! Populating `vendor/` from a download plan.
//!
//! Two halves: [`plan_install`], [`changed_versions`] and [`unverified_packages`] decide
//! *what* to fetch and touch no disk, while [`Vendor`] checks what is already there and does
//! the fetching. The split keeps the Installer free of download mechanics: it builds a plan
//! and hands it to [`Vendor`].

use crate::installer::path_linker::PathLinker;
use crate::output::Printer;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use phast_core::lock::phast::{PackageDef, PhastLock};
use phast_registry::http::downloader::Downloader;
use phast_workspace::graph;

use anyhow::{Context, Result};

/// Distinguishes a fresh install from a re-download so progress wording matches.
enum Action {
    /// Fresh package installation.
    Install,
    /// Re-download of an existing package at a newer version.
    Update,
}

#[rustfmt::skip]
impl Action {
    fn present(&self) -> &str {
        match self {
            Self::Install => "Installing",
            Self::Update  => "Updating",
        }
    }

    fn past(&self) -> &str {
        match self {
            Self::Install => "Installed",
            Self::Update  => "Updated",
        }
    }
}

/// Populates a `vendor/` directory from a set of locked packages.
///
/// Orchestrates two materialization strategies: the [`Downloader`] (shared
/// connection pool) fetches and extracts archive packages, and the [`PathLinker`]
/// symlinks local `path` packages. Callers only supply the packages.
pub struct Vendor {
    /// Coordinates fetching and extracting package archives.
    downloader: Downloader,
    /// Symlinks local `path` packages into `vendor/`.
    path_linker: PathLinker,
    /// Absolute path to the `vendor/` directory packages are placed into.
    vendor_dir: PathBuf,
    /// Progress and status output sink.
    printer: Printer,
}

impl Vendor {
    pub fn new(downloader: Downloader, vendor_dir: PathBuf, printer: Printer) -> Self {
        let path_linker = PathLinker::new(&vendor_dir);
        Self {
            downloader,
            path_linker,
            vendor_dir,
            printer,
        }
    }

    /// Download install-set packages not already present in `vendor/`.
    pub async fn install(&self, packages: Vec<PackageDef>) -> Result<()> {
        let to_install = self.missing(packages);

        if to_install.is_empty() {
            self.printer
                .info("Nothing to install, all packages already up to date.");
            return Ok(());
        }

        self.download(to_install, Action::Install).await
    }

    /// Download packages whose version changed, or whose directory is absent.
    ///
    /// `changed` carries the names whose resolved version differs from the
    /// previous lock; those are re-fetched even when already populated.
    pub async fn update(&self, packages: Vec<PackageDef>, changed: &HashSet<String>) -> Result<()> {
        let to_download: Vec<PackageDef> = packages
            .into_iter()
            .filter(|p| changed.contains(&p.name) || !self.is_populated(&p.name))
            .collect();

        if to_download.is_empty() {
            self.printer
                .hint("Nothing to update, all packages already up to date.");
            return Ok(());
        }

        self.download(to_download, Action::Update).await
    }

    /// Place `packages` into `vendor/`: symlink local `path` packages to their
    /// source, download and extract the rest.
    ///
    /// Archive downloads delegate to [`Downloader::download`], which runs requests
    /// concurrently and writes each archive to the disk cache before extracting.
    /// Returns an error if any single link, download, or extraction fails.
    #[rustfmt::skip]
    async fn download(&self, packages: Vec<PackageDef>, action: Action) -> Result<()> {
        let start = Instant::now();
        let count = packages.len();

        // Local path packages are symlinked to their source; the rest are downloaded.
        let (paths, archives): (Vec<PackageDef>, Vec<PackageDef>) = packages
            .into_iter()
            .partition(PackageDef::is_path);
        self.path_linker.link(&paths)?;

        // Surface any package the registry published without a checksum: it will
        // be installed with no integrity verification (see `verify_checksum`).

        // TODO: warn once per package installed without a checksum.
        //       Disabled until the warnings can be grouped: one line
        //       each floods a large install.
        // for name in unverified_packages(&archives) {
        //     self.printer.warning(hints::missing_checksum(name));
        // }

        let spinner = self.printer.spinner(&format!("{}...", action.present()));
        self.downloader
            .download(&archives, &self.vendor_dir, |name, ver| { spinner.tick(action.present(), name, ver) })
            .await
            .context("failed to download packages")?;
        spinner.finish_and_clear();

        self.printer.phase(action.past(), count, start.elapsed().as_secs_f64());

        Ok(())
    }

    /// The packages in `packages` that `vendor/` does not hold yet, which is what
    /// [`install`](Self::install) downloads, and what a dry run reports.
    pub fn missing(&self, packages: Vec<PackageDef>) -> Vec<PackageDef> {
        packages
            .into_iter()
            .filter(|p| !self.is_populated(&p.name))
            .collect()
    }

    /// Whether the package's directory exists and holds at least one entry.
    ///
    /// The version is not part of the question, so a directory left by an older release
    /// answers yes. That is why `install` on its own cannot notice a version change, and why
    /// `update` passes `changed` alongside the plan to force those packages through anyway.
    #[rustfmt::skip]
    fn is_populated(&self, name: &str) -> bool {
        let dir = self.vendor_dir.join(name);

        dir.is_dir() &&
        dir.read_dir().is_ok_and(|mut d| d.next().is_some())
    }
}

/// The external packages an install targets.
///
/// Without `member` this is every external (non-member) package in the lock,
/// including dev unless `no_dev`. With `member` it is that member's transitive
/// closure. Either way workspace members are excluded: they live in their
/// source trees and are never downloaded.
#[rustfmt::skip]
pub fn plan_install(lock: &PhastLock, member: Option<&str>, no_dev: bool) -> Vec<PackageDef> {
    if let Some(member) = member {
        return graph::closure(lock, member, !no_dev);
    }

    let mut packages = lock
        .packages
        .iter()
        .collect::<Vec<_>>();

    if !no_dev {
        packages.extend(lock.packages_dev.iter());
    }

    packages
        .into_iter()
        .filter(|p| !p.is_member())
        .cloned()
        .collect()
}

/// Names of packages that will be fetched without integrity verification:
/// they carry a `dist` to download but no (or an empty) `dist.shasum`.
///
/// A registry that omits the checksum disables verification for that package, see
/// `verify_checksum` in the downloader.
///
/// Nobody is warned about it today. The one call site sits commented out in `download`,
/// waiting on a way to report many packages without burying the rest of the output, so
/// outside its own test this function has no caller.
pub fn unverified_packages(packages: &[PackageDef]) -> Vec<&str> {
    packages
        .iter()
        .filter(|p| {
            p.dist
                .as_ref()
                .is_some_and(|d| d.shasum.as_deref().is_none_or(str::is_empty))
        })
        .map(|p| p.name.as_str())
        .collect()
}

/// Names whose resolved version differs between `old` and `new`.
///
/// A package absent from `old` (newly added) counts as changed. Used by the
/// update flow to force a re-download even when the package directory is
/// already populated with the previous version.
pub fn changed_versions(old: Option<&PhastLock>, new: &PhastLock) -> HashSet<String> {
    let locked: HashMap<&str, &str> = old
        .iter()
        .flat_map(|l| l.packages.iter().chain(l.packages_dev.iter()))
        .map(|p| (p.name.as_str(), p.version.as_str()))
        .collect();

    new.packages
        .iter()
        .chain(new.packages_dev.iter())
        .filter(|p| {
            locked
                .get(p.name.as_str())
                .is_none_or(|old| *old != p.version.as_str())
        })
        .map(|p| p.name.clone())
        .collect()
}
