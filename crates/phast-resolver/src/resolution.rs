//! The `Resolution` type: the output of a successful dependency resolution.
//!
//! A `Resolution` is an immutable snapshot of the (package, version) pairs that
//! are actually installed. It carries no source URLs, autoload maps or content
//! hashes; those are filled in by `phast-registry` during the lock file generation
//! that follows resolution.

use std::collections::{HashMap, HashSet};

use phast_core::package::PackageName;
use phast_version::Version;

/// The result of a successful resolution.
#[must_use]
#[derive(Debug)]
pub struct Resolution {
    /// Each package to install mapped to its resolved version, transitive ones
    /// included. The solver selects more than this holds: the synthetic root,
    /// the platform packages it treated as constraints, and any virtual package
    /// a real one satisfies are all dropped before the snapshot is built.
    pub packages: HashMap<PackageName, Version>,
    /// The subset of `packages` reachable only through a dev requirement.
    ///
    /// Derived by difference, from what a solve of every requirement holds that
    /// a solve of the production ones does not, so a package both sides need
    /// counts as production rather than being listed twice.
    pub dev_package_names: HashSet<PackageName>,
}

impl Resolution {
    /// Iterate over production packages only (excludes dev-only).
    pub fn production_packages(&self) -> impl Iterator<Item = (&PackageName, &Version)> {
        self.packages
            .iter()
            .filter(|(name, _)| !self.dev_package_names.contains(*name))
    }

    /// Iterate over dev packages only.
    pub fn dev_packages(&self) -> impl Iterator<Item = (&PackageName, &Version)> {
        self.packages
            .iter()
            .filter(|(name, _)| self.dev_package_names.contains(*name))
    }
}
