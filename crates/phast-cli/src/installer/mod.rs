//! The install pipeline: the [`Installer`] orchestrator and its phase services.
//!
//! Not the single entry point it is shaped like. Of the four phases it sequences only
//! `resolver` and `locker` are sealed: [`Autoloader`] is re-exported and `phast autoload`
//! drives it directly, dumping the autoloader with no [`Installer`] in the picture, and
//! [`vendor`], [`requirements`] and [`path_linker`] are public modules too, though nothing
//! outside this one imports them today.
//!
//! One [`Installer`] is built per command invocation. The `HttpClient` it is handed and the
//! cache root it is given both reach resolution and download, so credentials configured once
//! cover metadata and archives alike. All progress goes through `Printer`, so verbosity is
//! honored in every phase.

pub mod path_linker;
pub mod prune;
pub mod requirements;
pub mod vendor;

mod autoloader;
mod explain;
mod locker;
mod resolver;

use crate::hints;
use crate::output::Printer;

pub use autoloader::Autoloader;
use locker::Locker;
use prune::Pruner;
use resolver::{ResolveRequest, Resolver};
use vendor::Vendor;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use phast_autoload::AutoloadOptions;
use phast_core::PackageName;
use phast_core::lock::phast::{PhastLock, PhastLockState};
use phast_registry::http::cache::Cache;
use phast_registry::http::client::HttpClient;
use phast_registry::http::downloader::Downloader;
use phast_resolver::Requirement;
use phast_runtime::event::{LifecycleEvent, LifecycleEventManager};
use phast_version::Version;
use phast_version::parser::parse_version;
use phast_workspace::Workspace;

use anyhow::{Context, Result, bail};

/// How much of the workspace an install should download.
#[derive(Default)]
pub enum InstallScope {
    /// Download every external dependency in the lock (the whole workspace).
    #[default]
    Full,
    /// Download only one member's dependency closure, by its package name.
    /// The command layer resolves and validates the member (see `install.rs`).
    Member(String),
}

impl InstallScope {
    /// The member package name to scope to, or `None` for a full install.
    fn member(&self) -> Option<&str> {
        match self {
            Self::Full => None,
            Self::Member(name) => Some(name),
        }
    }
}

/// Which packages `update` may move.
#[derive(Default)]
pub enum UpdateScope {
    /// Every package in the lock.
    #[default]
    All,
    /// Only these; everything else keeps its locked version.
    Packages(Vec<PackageName>),
}

/// Options for [`Installer::lock`].
pub struct LockOptions {
    /// Re-resolve even if the lock file is already fresh.
    pub refresh: bool,
}

/// Options for [`Installer::add`].
pub struct AddOptions {
    /// The requirements to resolve, or `None` when every constraint is already known.
    ///
    /// `None` keeps an `add` with explicit constraints offline:
    /// nothing has to be chosen, so nothing is asked of a registry.
    pub requirements: Option<Vec<Requirement>>,
}

/// Options for [`Installer::install`].
#[derive(Default)]
pub struct InstallOptions {
    /// Exclude dev dependencies.
    pub no_dev: bool,
    /// Print what would change without writing anything.
    pub dry_run: bool,
    /// Force classmap-authoritative mode.
    pub authoritative: bool,
    /// How much of the workspace to download.
    pub scope: InstallScope,
}

/// Options for [`Installer::update`].
pub struct UpdateOptions {
    /// Exclude dev dependencies.
    pub no_dev: bool,
    /// Print what would change without writing anything.
    pub dry_run: bool,
    /// Which packages may move.
    pub scope: UpdateScope,
}

/// Service that coordinates the full install lifecycle.
///
/// Receives a fully-configured `HttpClient` (with auth baked in) at construction
/// time. The `Downloader` is built from that client once and shared across all
/// download phases.
pub struct Installer {
    /// Runs the dependency solver against the manifest.
    resolver: Resolver,
    /// Reads, staleness-checks, and writes `phast.lock`.
    locker: Locker,
    /// Fetches package archives into `vendor/`.
    vendor: Vendor,
    /// Empties `vendor/` of what left the lock.
    pruner: Pruner,
    /// Generates `vendor/autoload.php` and `vendor/bin/`.
    autoloader: Autoloader,
    /// Dispatches lifecycle events to registered handlers.
    lifecycle: LifecycleEventManager,
    /// Workspace context; carries the root path, resolved manifest, and any declared members.
    workspace: Workspace,
    /// Progress and status output sink.
    printer: Printer,
}

#[rustfmt::skip]
impl Installer {
    /// Assemble the four phases for one command invocation.
    ///
    /// `registry_url` replaces every `[[repositories]]` entry with a single endpoint, and
    /// `None` keeps them. Only the `None` path is given a warning sink, so naming a registry
    /// also silences the complaint about a repository type Phast cannot use.
    ///
    /// `phast.lock` and `vendor/` are both derived from `workspace.root()`, so narrowing a
    /// command to one member never moves either of them.
    pub fn new(
        registry_url: Option<String>,
        http_client: HttpClient,
        cache_dir: PathBuf,
        lifecycle: LifecycleEventManager,
        workspace: Workspace,
        printer: Printer,
    ) -> Self {
        let lock_path  = workspace.root().join("phast.lock");
        let vendor_dir = workspace.root().join("vendor");

        let downloader = Downloader::new(http_client.clone(), Cache::new(&cache_dir));
        let resolver   = Resolver::new(registry_url, http_client, cache_dir, printer.clone());
        let locker     = Locker::new(lock_path, printer.clone());
        let vendor     = Vendor::new(downloader, vendor_dir.clone(), printer.clone());
        let pruner     = Pruner::new(vendor_dir.clone(), printer.clone());
        let autoloader = Autoloader::new(vendor_dir, lifecycle.clone(), printer.clone());

        Self {
            resolver,
            locker,
            vendor,
            pruner,
            autoloader,
            lifecycle,
            workspace,
            printer,
        }
    }

    /// Resolve the manifest and write `phast.lock` without downloading anything.
    pub async fn lock(&mut self, options: LockOptions) -> Result<()> {
        let start = Instant::now();

        let hash = self.locker.content_hash(&self.workspace)?;
        let lock = if options.refresh {
            // Bypass the freshness check but still pass the existing lock so
            // the solver can build a preferences map from it. This keeps already-locked
            // versions stable: the solver picks them first and only deviates when a
            // constraint forces a change. Without this, --refresh would pick the highest
            // available version for every package and produce unnecessary upgrades.
            self.locker.read()
        } else {
            match self.locker.load(&hash) {
                PhastLockState::Fresh(_) => return Ok(()),
                PhastLockState::Stale(l) => Some(l),
                PhastLockState::Missing  => None,
            }
        };

        let request  = ResolveRequest::default();
        let response = self
            .resolver
            .resolve(lock, request, &self.workspace)
            .await?;
        let lock = self
            .locker
            .write(response, &hash, &self.workspace)
            .await?;

        let total = lock.packages.len() + lock.packages_dev.len();
        self.printer.summary(total, start.elapsed().as_secs_f64());

        Ok(())
    }

    /// Resolve the added requirements and record them, leaving the install to the caller.
    ///
    /// `record` writes them to the manifest and reports whether it wrote anything, and
    /// receives the versions the solver chose for any requirement that arrived without a
    /// constraint. It runs after resolution, so a failure leaves the manifest untouched.
    pub async fn add(
        &mut self,
        options: AddOptions,
        record: impl FnOnce(&HashMap<PackageName, Version>) -> Result<bool>,
    ) -> Result<bool> {
        let Some(requirements) = options.requirements else {
            // Nothing to choose, so nothing to resolve.
            let recorded = record(&HashMap::new())?;
            if !recorded { return Ok(false); }

            // Re-read what `record` wrote, or a following install
            // hashes the old manifest and reads a stale lock as fresh.
            self.workspace = Workspace::resolve(self.workspace.root())?;

            return Ok(recorded);
        };

        let request  = ResolveRequest { proposed: requirements, ..Default::default() };
        let response = self
            .resolver
            .resolve(self.locker.read(), request, &self.workspace)
            .await?;
        if !record(&response.resolution.packages)? {
            return Ok(false);
        }

        // The hash below must describe the manifest as recorded, not as solved:
        // the two differ by the constraint just written.
        self.workspace = Workspace::resolve(self.workspace.root())?;

        let hash = self.locker
            .content_hash(&self.workspace)?;
        self.locker
            .write(response, &hash, &self.workspace)
            .await?;

        Ok(true)
    }

    /// Run a full install cycle: resolve if needed, download missing packages, generate autoloader.
    ///
    /// Used by `phast install`, `phast add`, and `phast remove`.
    pub async fn install(&mut self, options: InstallOptions) -> Result<()> {
        let start = Instant::now();

        // A dry run reports and nothing else: no hook runs, since a hook
        // is the project's own code, and nothing is written.
        if !options.dry_run {
            self.lifecycle.fire(LifecycleEvent::PreInstallCmd)?;
        }

        let hash = self.locker.content_hash(&self.workspace)?;
        let lock = match self.locker.load(&hash) {
            PhastLockState::Fresh(lock) => {
                let plan = vendor::plan_install(
                    &lock,
                    options.scope.member(),
                    options.no_dev,
                );

                if options.dry_run {
                    let to_install = self.vendor.missing(plan);
                    self.print_dry_run(
                        to_install
                            .iter()
                            .map(|p| p.name.as_str())
                            .collect(),
                    );
                    return Ok(());
                }

                self.vendor.install(plan).await?;

                self.prune(&lock, options.no_dev)?;
                self.autoload(&lock, options.no_dev, options.authoritative)?;

                self.lifecycle.fire(LifecycleEvent::PostInstallCmd)?;

                let total = lock.packages.len() + lock.packages_dev.len();
                self.printer.summary(total, start.elapsed().as_secs_f64());

                return Ok(());
            }
            PhastLockState::Stale(s) => Some(s),
            PhastLockState::Missing  => None,
        };

        let request  = ResolveRequest::default();
        let response = self
            .resolver
            .resolve(lock, request, &self.workspace)
            .await?;

        if options.dry_run {
            self.print_dry_run(
                response
                    .resolution
                    .packages
                    .keys()
                    .map(PackageName::as_str)
                    .collect(),
            );
            return Ok(());
        }

        let lock = self
            .locker
            .write(response, &hash, &self.workspace)
            .await?;
        let plan = vendor::plan_install(
            &lock,
            options.scope.member(),
            options.no_dev,
        );
        self.vendor.install(plan).await?;

        self.prune(&lock, options.no_dev)?;
        self.autoload(&lock, options.no_dev, options.authoritative)?;

        self.lifecycle.fire(LifecycleEvent::PostInstallCmd)?;

        let total = lock.packages.len() + lock.packages_dev.len();
        self.printer.summary(total, start.elapsed().as_secs_f64());

        Ok(())
    }

    /// Re-resolve packages to their latest allowed versions, download changed ones, regenerate autoloader.
    ///
    /// [`UpdateScope::All`] re-resolves everything freely. Otherwise only the named
    /// packages are updated; all others are pinned to their locked versions.
    pub async fn update(&mut self, options: UpdateOptions) -> Result<()> {
        let start = Instant::now();

        let old_lock = self.locker.read();
        let pins = self.build_locked_pins(old_lock.as_ref(), &options.scope)?;

        // A dry run reports and nothing else: no hook runs, since a hook
        // is the project's own code, and nothing is written.
        if !options.dry_run {
            self.lifecycle.fire(LifecycleEvent::PreUpdateCmd)?;
        }

        let request  = ResolveRequest { pins, ..Default::default() };
        let response = self
            .resolver
            .resolve(None, request, &self.workspace)
            .await?;

        if options.dry_run {
            self.print_dry_run(
                response
                    .resolution
                    .packages
                    .keys()
                    .map(PackageName::as_str)
                    .collect(),
            );
            return Ok(());
        }

        let hash = self.locker
            .content_hash(&self.workspace)?;
        let lock = self.locker
            .write(response, &hash, &self.workspace)
            .await?;

        let planned = vendor::plan_install(&lock, None, options.no_dev);
        let changed = vendor::changed_versions(old_lock.as_ref(), &lock);
        self.vendor.update(planned, &changed).await?;

        self.prune(&lock, options.no_dev)?;
        self.autoload(&lock, options.no_dev, false)?;

        self.lifecycle.fire(LifecycleEvent::PostUpdateCmd)?;

        let total = lock.packages.len() + lock.packages_dev.len();
        self.printer.summary(total, start.elapsed().as_secs_f64());

        Ok(())
    }

    /// Run the prune phase: remove from `vendor/` what the lock no longer keeps.
    ///
    /// The whole lock decides, never a member's closure, so a scoped install keeps what
    /// another member uses.
    fn prune(&self, lock: &PhastLock, no_dev: bool) -> Result<()> {
        let kept = vendor::plan_install(lock, None, no_dev);
        self.pruner.retain(&kept)
    }

    /// Run the autoload phase: regenerate the autoloader
    /// (the loader wraps it in its own lifecycle hooks) and link bin stubs.
    ///
    /// `authoritative` forces classmap-authoritative mode on, and the manifest's
    /// `[config]` can force it on independently, so this flag can enable it but not disable it.
    fn autoload(&mut self, lock: &PhastLock, no_dev: bool, authoritative: bool) -> Result<()> {
        let classmap_authoritative = authoritative || self
            .workspace
            .manifest()
            .config
            .classmap_authoritative;

        let options = AutoloadOptions {
            include_dev: !no_dev,
            authoritative: classmap_authoritative,
        };

        self.autoloader.generate(lock, options, &self.workspace)
    }

    /// Build the `locked_pins` map for selective update.
    ///
    /// Pins every locked package except the named ones, so only targeted packages
    /// are re-resolved. Pins nothing for [`UpdateScope::All`].
    fn build_locked_pins(
        &self,
        lock: Option<&PhastLock>,
        scope: &UpdateScope,
    ) -> Result<HashMap<PackageName, Version>> {
        let UpdateScope::Packages(packages) = scope else {
            return Ok(HashMap::new());
        };

        let lock = lock
            .context("phast.lock not found. Run `phast install` first")?;

        for package in packages {
            let found = lock
                .packages
                .iter()
                .chain(lock.packages_dev.iter())
                .any(|p| PackageName::new(&p.name) == *package);
            if !found {
                bail!(hints::package_not_in_lock(package.as_str()));
            }
        }

        let pins = lock
            .packages
            .iter()
            .chain(lock.packages_dev.iter())
            .filter_map(|p| Some((PackageName::new(&p.name), parse_version(&p.version).ok()?)))
            .filter(|(name, _)| !packages.contains(name))
            .collect();

        Ok(pins)
    }

    /// Print a dry-run summary listing `packages`, the ones that would be installed.
    fn print_dry_run(&self, mut packages: Vec<&str>) {
        packages.sort_unstable();

        if packages.is_empty() {
            self.printer.info("Nothing to install, all packages already up to date.");
        }
        for package in &packages {
            self.printer.info(format!("  - Would install {package}"));
        }

        self.printer.info("\nDry run -- no files written.");
    }
}
