//! The platform a dependency set requires at runtime.
//!
//! Aggregates the `php` version floor and `ext-*` requirements declared across
//! the root manifest *and* every locked package, so a generated
//! `platform_check.php` guard can fail fast when the runtime does not satisfy
//! them. Aggregating the whole tree (not just the root) is deliberate:
//! a runtime missing an extension a dependency needs would otherwise boot
//! and only fail deep inside that dependency later.

use crate::lock::phast::{PackageDef, PhastLock};
use crate::manifest::{Manifest, PlatformCheck};
use crate::package::{is_ext, is_php};

use std::collections::BTreeSet;

use phast_version::parser::parse_constraint;
use phast_version::{Constraint, RequireSpec, Version};

/// The platform requirements a dependency set declares.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PlatformRequirements {
    /// The highest minimum `PHP_VERSION_ID` any package requires
    /// (e.g. `80100` for 8.1.0), or `None` when no `php` constraint has a floor.
    pub php: Option<u64>,
    /// Required extension names, without the `ext-` prefix, sorted and unique.
    pub extensions: BTreeSet<String>,
}

impl PlatformRequirements {
    /// Aggregate the requirements the guard should verify, honoring the
    /// manifest's `platform-check` mode and whether dev packages count.
    /// Returns `None` when the guard is disabled ([`PlatformCheck::Off`]);
    /// extensions are collected only in [`PlatformCheck::All`].
    ///
    /// `include_dev` reaches the locked dev packages only. The root contributes its
    /// `require` either way and its `require-dev` never.
    pub fn collect(lock: &PhastLock, manifest: &Manifest, include_dev: bool) -> Option<Self> {
        let with_extensions = match manifest.config.platform_check {
            PlatformCheck::Off => return None,
            PlatformCheck::Php => false,
            PlatformCheck::All => true,
        };

        let mut requirements = Self::default();

        requirements.add_root(manifest, with_extensions);
        requirements.add_packages(lock, include_dev, with_extensions);

        Some(requirements)
    }

    /// Fold the root project's own platform requirements.
    fn add_root(&mut self, manifest: &Manifest, with_extensions: bool) {
        for (name, spec) in &manifest.require {
            let RequireSpec::Constraint { constraint, .. } = spec else {
                continue;
            };

            self.add_requirement(name.as_str(), constraint, with_extensions);
        }
    }

    /// Fold every locked package's platform requirements, dev included when asked.
    fn add_packages(&mut self, lock: &PhastLock, include_dev: bool, with_extensions: bool) {
        let dev_packages: &[PackageDef] = if include_dev { &lock.packages_dev } else { &[] };
        for package in lock.packages.iter().chain(dev_packages) {
            for (name, raw) in &package.require.entries {
                let Ok(constraint) = parse_constraint(raw) else {
                    continue;
                };

                self.add_requirement(name, &constraint, with_extensions);
            }
        }
    }

    /// Fold one requirement in: raise the php floor, or record an extension.
    fn add_requirement(&mut self, name: &str, constraint: &Constraint, with_extensions: bool) {
        if is_php(name) {
            self.php = self.php.max(php_version_id(constraint.lower_bound()));
            return;
        }

        if is_ext(name) && with_extensions {
            let extension = name.strip_prefix("ext-").unwrap_or(name);
            self.extensions.insert(extension.to_owned());
        }
    }
}

/// The `PHP_VERSION_ID` integer for a version floor (`major*10000 + minor*100 +
/// patch`). `None` for a branch version or an absent floor.
fn php_version_id(version: Option<Version>) -> Option<u64> {
    match version? {
        Version::Numeric(v) => Some(v.segments[0] * 10000 + v.segments[1] * 100 + v.segments[2]),
        Version::Branch(_) => None,
    }
}
