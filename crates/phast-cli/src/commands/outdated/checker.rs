//! Querying the registry for newer releases of the locked packages.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use phast_core::manifest::Manifest;
use phast_core::package::PackageName;
use phast_registry::http::client::HttpClient;
use phast_registry::provider::ChainedProvider;
use phast_registry::router::RegistryOptions;
use phast_resolver::provider::Provider;
use phast_resolver::ranges::constraint_to_range;
use phast_version::parser::parse_version;
use phast_version::{Constraint, RequireSpec, Version};

use anyhow::Result;
use futures::future::join_all;
use serde::Serialize;

/// How the available update relates to the locked version.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateKind {
    /// New patch release: same major and minor, higher patch.
    Patch,
    /// New minor release: same major, higher minor.
    Minor,
    /// New major release or latest is outside the locked constraint.
    Major,
}

impl UpdateKind {
    /// Lower-case label for text output.
    ///
    /// JSON does not use this: `rename_all` on the enum produces the same strings
    /// by coincidence, so changing one here will not change the other.
    pub fn as_str(self) -> &'static str {
        match self {
            UpdateKind::Patch => "patch",
            UpdateKind::Minor => "minor",
            UpdateKind::Major => "major",
        }
    }
}

/// One outdated package entry.
#[derive(Debug, Serialize)]
pub struct OutdatedEntry {
    pub package: String,
    pub locked: String,
    pub latest: String,
    pub kind: UpdateKind,
}

/// Determine how the `latest` release relates to the `locked` version.
///
/// `Major` when a constraint was given and `latest` falls outside it, when the major
/// segment increased, or when either version is a branch rather than a number: a branch
/// has no major or minor to compare against, so it falls through to `Major`.
///
/// `Minor` when the major matches and the minor increased, and `Patch` for anything else.
/// The patch segment itself is never compared.
#[rustfmt::skip]
pub fn determine_kind(
    locked: &Version,
    latest: &Version,
    constraint: Option<&Constraint>,
) -> UpdateKind {
    if let Some(c) = constraint && !constraint_to_range(c).contains(latest) {
        return UpdateKind::Major;
    }

    match (locked, latest) {
        (Version::Numeric(l), Version::Numeric(t)) => {
            if t.segments[0] > l.segments[0] { return UpdateKind::Major; }
            if t.segments[1] > l.segments[1] { return UpdateKind::Minor; }
            UpdateKind::Patch
        }
        _ => UpdateKind::Major,
    }
}

/// Queries the registry and returns entries for packages that have newer versions available.
pub struct OutdatedChecker {
    provider: ChainedProvider,
    constraints: HashMap<PackageName, Constraint>,
}

impl OutdatedChecker {
    /// Build a checker from a manifest.
    ///
    /// `base` is the project root, used to resolve any `path` repositories.
    /// `on_warning` is called for each unsupported repository type encountered
    /// while building the provider chain.
    #[rustfmt::skip]
    pub async fn new(
        base: &Path,
        manifest: &Manifest,
        client: HttpClient,
        cache_dir: PathBuf,
        on_warning: impl FnMut(&str),
    ) -> Result<Self> {
        let options  = RegistryOptions::from_manifest(manifest);
        let provider = ChainedProvider::new(client, cache_dir)
            .with_manifest(base, manifest, options, on_warning)
            .await?;

        // A `self.version` dependency is pinned to the local monorepo version, so
        // it is not a candidate for "outdated" against a registry;
        // skip those and keep the concrete constraints.
        let constraints = manifest
            .require
            .iter()
            .chain(manifest.require_dev.iter())
            .filter_map(|(name, spec)| match spec {
                RequireSpec::Constraint { constraint, .. } => Some((name.clone(), constraint.clone())),
                RequireSpec::SelfVersion => None,
            })
            .collect();

        Ok(Self { provider, constraints })
    }

    /// Check `packages` against the registry and return outdated entries.
    ///
    /// `packages` is a list of `(name, locked_version)` pairs, typically from
    /// [`lock::collect`](crate::project::lock::collect).
    #[rustfmt::skip]
    pub async fn check(&self, packages: &[(String, String)]) -> Result<Vec<OutdatedEntry>> {
        let prefetch_names: Vec<PackageName> = packages
            .iter()
            .map(|(name, _)| PackageName::new(name))
            .collect();
        self.provider.prefetch(&prefetch_names).await;

        let futures = packages.iter().map(|(name, version)| {
            let name    = PackageName::new(name);
            let version = version.clone();

            async move {
                let versions = self
                    .provider
                    .versions(&name)
                    .await
                    .unwrap_or_default();
                (name, version, versions)
            }
        });
        let fetched = join_all(futures).await;

        let mut outdated = Vec::new();

        for (name, version, versions) in fetched {
            // No versions available from the registry; skip rather than report as outdated.
            let Some(latest) = versions.into_iter().next() else {
                continue;
            };
            // Locked version string is unparseable; skip to avoid a spurious report.
            let Ok(locked) = parse_version(&version) else {
                continue;
            };
            // Already at the latest available version; nothing to report.
            if locked == latest {
                continue;
            }

            let kind = determine_kind(&locked, &latest, self.constraints.get(&name));

            outdated.push(OutdatedEntry {
                package: name.as_str().to_owned(),
                locked:  version,
                latest:  latest.to_string(),
                kind,
            });
        }

        Ok(outdated)
    }
}
