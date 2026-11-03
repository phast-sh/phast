//! In-memory `Provider` implementation for tests.
//!
//! Build a fixture universe, populate it, then hand it to `resolve()` in place of
//! a real registry.
//!
//! ```
//! use phast_resolver::provider::memory::MemoryProvider;
//! use phast_version::parser::{parse_constraint, parse_version};
//!
//! let v3 = parse_version("3.0.0").unwrap();
//! let mut provider = MemoryProvider::new();
//!
//! provider.add_package("monolog/monolog", v3.clone(), vec![
//!     ("psr/log".into(), parse_constraint("^3.0").unwrap()),
//! ]);
//! provider.add_package_full("psr/log", v3.clone(), vec![], vec![
//!     ("psr/log-implementation".into(), v3),
//! ], vec![]);
//! ```

use crate::provider::{PackageMetadata, Provider};

use std::collections::HashMap;
use std::future::{Future, ready};

use phast_core::package::PackageName;
use phast_version::{Constraint, Version};

/// Shorthand for a list of parsed (package, constraint) dependency pairs.
type Deps = Vec<(PackageName, Constraint)>;
/// Shorthand for a list of (package, version) provide/replace entries.
type NamedVersions = Vec<(PackageName, Version)>;

/// One registered version of a package.
struct VersionEntry {
    deps: Deps,
    provide: NamedVersions,
    replace: NamedVersions,
}

/// In-memory [`Provider`] for use in tests and benchmarks.
///
/// A fixture can express `require`, `provide` and `replace`, but never
/// `require-dev`: metadata always comes back with an empty one, since a
/// dependency's dev requirements are not resolved anyway.
#[derive(Clone, Default)]
pub struct MemoryProvider {
    packages: HashMap<PackageName, Vec<(Version, VersionEntry)>>,
}

impl MemoryProvider {
    /// Create an empty provider with no packages registered.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a version with its direct dependencies and no provide/replace.
    pub fn add_package(
        &mut self,
        name: impl Into<String>,
        version: Version,
        deps: Vec<(String, Constraint)>,
    ) {
        self.add_package_full(name, version, deps, vec![], vec![]);
    }

    /// Register a version with direct dependencies, provide declarations, and replace declarations.
    pub fn add_package_full(
        &mut self,
        name: impl Into<String>,
        version: Version,
        deps: Vec<(String, Constraint)>,
        provide: Vec<(String, Version)>,
        replace: Vec<(String, Version)>,
    ) {
        let name = PackageName::new(name);
        let entry = VersionEntry {
            deps: deps
                .into_iter()
                .map(|(n, c)| (PackageName::new(n), c))
                .collect(),
            provide: provide
                .into_iter()
                .map(|(n, v)| (PackageName::new(n), v))
                .collect(),
            replace: replace
                .into_iter()
                .map(|(n, v)| (PackageName::new(n), v))
                .collect(),
        };
        self.packages
            .entry(name)
            .or_default()
            .push((version, entry));
    }
}

impl Clone for VersionEntry {
    fn clone(&self) -> Self {
        Self {
            deps: self.deps.clone(),
            provide: self.provide.clone(),
            replace: self.replace.clone(),
        }
    }
}

impl Provider for MemoryProvider {
    /// Return all registered versions for `name`, newest first.
    ///
    /// A name nobody registered comes back empty rather than failing, which is how
    /// a fixture makes a package absent and sends the solve down the virtual path.
    fn versions(
        &self,
        name: &PackageName,
    ) -> impl Future<Output = anyhow::Result<Vec<Version>>> + Send {
        let mut result: Vec<Version> = self
            .packages
            .get(name)
            .map(|p| p.iter().map(|(v, _)| v.clone()).collect())
            .unwrap_or_default();
        result.sort_unstable_by(|a, b| b.cmp(a));
        ready(Ok(result))
    }

    /// Return the dependency metadata for `name` at `version`.
    ///
    /// Unlike `versions`, an unregistered pair is an error: asking for metadata
    /// means the solve already chose that version, so its absence is a broken
    /// fixture rather than a fact about the package.
    fn metadata(
        &self,
        name: &PackageName,
        version: &Version,
    ) -> impl Future<Output = anyhow::Result<PackageMetadata>> + Send {
        let result = self
            .packages
            .get(name)
            .and_then(|p| p.iter().find(|(v, _)| v == version))
            .map(|(_, entry)| PackageMetadata {
                name: name.clone(),
                version: version.clone(),
                require: entry.deps.clone(),
                require_dev: vec![],
                provide: entry.provide.clone(),
                replace: entry.replace.clone(),
            })
            .ok_or_else(|| {
                anyhow::anyhow!("package {}@{} not found in memory provider", name, version)
            });

        ready(result)
    }
}
