//! Where autoload rules come from, and in what order.
//!
//! Both the full maps and the PSR rules are collected from here, so they never disagree
//! about which packages are included.

use std::path::{Path, PathBuf};

use phast_core::lock::phast::{AutoloadSection, PackageDef, PhastLock};
use phast_workspace::MemberAutoload;

use anyhow::Result;

/// Collects autoload rules, one source at a time.
///
/// It lets the same walk over the sources fill either the full maps or just the PSR rules.
///
/// Implementors: `Maps` (the full maps), `Rules` (PSR-4 and PSR-0 only).
pub trait Collect {
    /// What you get back once collecting is done.
    type Output;

    /// Add the rules from `autoload`. Its paths are relative to `dir`.
    fn add(&mut self, autoload: &AutoloadSection, dir: &Path) -> Result<()>;

    /// Return what was collected. Called after the last source.
    fn finish(self) -> Self::Output;
}

/// Walks a project's autoload sources in order and feeds them to a collector.
pub struct Sources<C: Collect> {
    /// Where installed packages live, each as `<vendor>/<name>`.
    /// The root package is the directory above it.
    vendor_dir: PathBuf,
    /// Whether to include dev packages and dev autoload rules.
    include_dev: bool,
    /// The collector every source goes to.
    collector: C,
}

impl<C: Collect> Sources<C> {
    /// Create a plan for the packages in `vendor_dir` that feeds `collector`.
    pub fn new(vendor_dir: &Path, include_dev: bool, collector: C) -> Self {
        Self {
            vendor_dir: vendor_dir.to_path_buf(),
            include_dev,
            collector,
        }
    }

    /// Feed every source to the collector, then finish it.
    ///
    /// Dependencies come first, so a package's `files` load after the files
    /// of the packages it needs.
    pub fn collect(mut self, lock: &PhastLock, members: &[MemberAutoload]) -> Result<C::Output> {
        self.packages(lock)?;
        self.members(members)?;
        self.root(lock)?;
        Ok(self.collector.finish())
    }

    /// Add the installed packages, in lock order. A package only brings
    /// its production rules.
    fn packages(&mut self, lock: &PhastLock) -> Result<()> {
        for package in &lock.packages {
            self.package(package)?;
        }
        if self.include_dev {
            for package in &lock.packages_dev {
                self.package(package)?;
            }
        }
        Ok(())
    }

    /// Add one installed package. Its paths start at `vendor/<name>`.
    fn package(&mut self, package: &PackageDef) -> Result<()> {
        let dir = self.vendor_dir.join(&package.name);
        self.collector.add(&package.autoload, &dir)
    }

    /// Add the workspace members, in the order they're declared.
    fn members(&mut self, members: &[MemberAutoload]) -> Result<()> {
        for member in members {
            self.member(member)?;
        }
        Ok(())
    }

    /// Add one workspace member, plus its dev rules when dev is included.
    fn member(&mut self, member: &MemberAutoload) -> Result<()> {
        self.collector.add(&member.autoload, &member.dir)?;
        if self.include_dev {
            self.collector.add(&member.autoload_dev, &member.dir)?;
        }
        Ok(())
    }

    /// Add the root package. Its paths start at the project directory, above `vendor/`.
    #[rustfmt::skip]
    fn root(&mut self, lock: &PhastLock) -> Result<()> {
        let root = self
            .vendor_dir
            .parent()
            .unwrap_or(&self.vendor_dir);

        self.collector.add(&lock.autoload, root)?;
        if self.include_dev {
            self.collector.add(&lock.autoload_dev, root)?;
        }
        Ok(())
    }
}
