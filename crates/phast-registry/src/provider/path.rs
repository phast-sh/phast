//! [`PathProvider`]: a package in a local directory, from a `path` repository
//! or a `[sources]` spec.

use crate::hints;
use crate::p2::convert::convert_extra;
use crate::provider::Lockable;

use std::path::Path;

use phast_core::git;
use phast_core::lock::phast::{AutoloadSection, DistSection, DistType, PackageDef};
use phast_core::manifest::Manifest;
use phast_core::package::PackageName;
use phast_core::shared::Author;
use phast_resolver::provider::{PackageMetadata, Provider};
use phast_version::Version;
use phast_version::parser::parse_declared_version;

use anyhow::Context;

/// A single package resolved from a `path` repository: code in a local directory.
///
/// Unlike [`RegistryProvider`](crate::provider::RegistryProvider), this does
/// no network or cache I/O. It loads the package manifest from disk once at
/// construction (via [`Manifest::load`], honoring the usual `phast.toml` →
/// `composer.json` precedence) and answers resolver queries for that one package
/// from the parsed data. The package is materialized into `vendor/` by symlink
/// rather than downloaded, so its lock entry carries [`DistType::Path`] with the
/// source directory in `url`.
#[derive(Clone, Debug)]
pub struct PathProvider {
    /// The package's declared `vendor/package` name (lowercased).
    name: PackageName,
    /// The package's declared, parsed version.
    version: Version,
    /// Source directory as declared in the repository entry, relative to the
    /// project root. Stored verbatim into the lock's `dist.url`.
    rel_path: String,
    /// The package's manifest (require, autoload, type, ...).
    manifest: Manifest,
}

#[rustfmt::skip]
impl PathProvider {
    /// The package name this provider answers for.
    pub fn name(&self) -> &PackageName {
        &self.name
    }

    /// Load the package at `base/rel_path` and build a provider for it.
    ///
    /// `base` is the directory of the manifest that declared the source (the
    /// project root), so a relative `rel_path` resolves correctly. The package
    /// must declare a `name`.
    ///
    /// `declared_version` is the version pinned by the `[sources]` entry, if any.
    /// It wins, matching Composer's `options.versions`; the package's own manifest
    /// version is the fallback when the source declares none. Failing both, the
    /// branch of the containing checkout becomes a `dev-<branch>`, which is how
    /// a version-less monorepo component is pinned at all. Outside a git checkout
    /// there is no third answer and loading fails.
    pub fn load(
        base: &Path,
        rel_path: &str,
        declared_version: Option<&str>,
    ) -> anyhow::Result<Self> {
        let manifest = Manifest::load(&base.join(rel_path))
            .with_context(|| format!("path repository '{rel_path}'"))?;

        let name = manifest
            .name
            .clone()
            .map(|n| PackageName::new(&n))
            .ok_or_else(|| anyhow::anyhow!(hints::path_without_name(rel_path)))?;

        // Version precedence: the [sources] pin, then the package's own manifest,
        // then a `dev-<branch>` guess from the containing checkout (a version-less monorepo component).
        // None of these means the package cannot be pinned.
        let version = match declared_version
            .map(str::to_owned)
            .or_else(|| manifest.version.clone())
        {
            Some(declared) => parse_declared_version(&declared)
                .with_context(|| format!("invalid version for path package '{}'", name.as_str()))?,
            None => git::current_branch(base)
                .map(Version::Branch)
                .ok_or_else(|| anyhow::anyhow!(hints::path_without_version(rel_path, name.as_str())))?,
        };

        Ok(Self {
            name,
            version,
            rel_path: rel_path.to_owned(),
            manifest,
        })
    }

    /// Build the locked package entry for this path package.
    ///
    /// Records a [`DistType::Path`] dist with the source directory in `url`;
    /// the package is materialized by symlink, not downloaded, so there is
    /// no `reference` or `shasum`. `path` is left unset so the entry is never
    /// mistaken for a workspace member.
    fn locked(&self) -> PackageDef {
        let require = self
            .manifest
            .require
            .iter()
            .map(|(name, constraint)| (name.as_str().to_owned(), constraint.to_string()))
            .collect();

        let dist = DistSection {
            dist_type: DistType::Path,
            url: self.rel_path.clone(),
            reference: None,
            shasum: None,
        };

        let autoload = AutoloadSection {
            psr4: self.manifest.autoload.psr4.clone(),
            psr0: self.manifest.autoload.psr0.clone(),
            classmap: self.manifest.autoload.classmap.clone(),
            files: self.manifest.autoload.files.clone(),
            exclude: self.manifest.autoload.exclude.clone(),
        };

        PackageDef {
            name: self.name.as_str().to_owned(),
            version: self.version.to_string(),
            path: None,
            require,
            require_dev: Default::default(),
            dist: Some(dist),
            source: None,
            package_type: self.manifest.package_type.clone(),
            description: self.manifest.description.clone(),
            homepage: None,
            time: None,
            notification_url: None,
            license: self.manifest.license.clone().into_iter().collect(),
            keywords: vec![],
            authors: Vec::<Author>::new(),
            support: Default::default(),
            extra: convert_extra(self.manifest.extra.clone()),
            autoload,
            provide: Default::default(),
            bin: vec![],
        }
    }
}

/// Answers from the manifest read at construction, never from the network.
///
/// A path package is pinned in the routing table under the name it declares,
/// so it never joins the fallback chain and is only ever asked about itself.
impl Provider for PathProvider {
    /// Return the single declared version when `name` matches, else empty.
    async fn versions(&self, name: &PackageName) -> anyhow::Result<Vec<Version>> {
        if name == &self.name {
            Ok(vec![self.version.clone()])
        } else {
            Ok(vec![])
        }
    }

    /// Return the package's `require`, `replace` and `provide` from its manifest.
    ///
    /// Dev dependencies are not included: like any installed dependency, a path
    /// package's `require-dev` is never resolved.
    ///
    /// Each spec resolves against this package's own version, so a monorepo sibling
    /// pin lands the same way a registry package's would.
    async fn metadata(
        &self,
        name: &PackageName,
        _version: &Version,
    ) -> anyhow::Result<PackageMetadata> {
        let require = self
            .manifest
            .require
            .iter()
            .map(|(n, s)| (n.clone(), s.resolve(&self.version)))
            .collect();
        let replace = self
            .manifest
            .replace
            .iter()
            .map(|(n, s)| (n.clone(), s.resolve(&self.version)))
            .collect();
        let provide = self
            .manifest
            .provide
            .iter()
            .map(|(n, s)| (n.clone(), s.resolve(&self.version)))
            .collect();

        Ok(PackageMetadata {
            name: name.clone(),
            version: self.version.clone(),
            require,
            require_dev: vec![],
            replace,
            provide,
        })
    }
}

/// A path package is already on disk, so its entry needs no I/O and cannot fail
/// beyond the name check: `version` is ignored, since the provider offers exactly one.
impl Lockable for PathProvider {
    async fn lock(&self, name: &PackageName, _version: &Version) -> anyhow::Result<PackageDef> {
        if name != &self.name {
            anyhow::bail!(hints::wrong_locked_package(
                self.name.as_str(),
                name.as_str(),
            ));
        }

        Ok(self.locked())
    }
}
