//! [`ChainedProvider`]: one `Provider` over every configured source.
//!
//! Build it with [`with_manifest`](ChainedProvider::with_manifest), or
//! [`with_url`](ChainedProvider::with_url) for a single-registry override
//! (`--registry-url`).

use crate::filter::Rejection;
use crate::http::client::HttpClient;
use crate::provider::Lockable;
use crate::router::{PackageProvider, RegistryOptions, RegistryRouter, RoutedProvider};

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use phast_core::lock::phast::{AutoloadSection, CURRENT_VERSION, MetadataSection, PhastLock};
use phast_core::lock::sort;
use phast_core::manifest::{Manifest, Repository};
use phast_core::meta::generated_by;
use phast_core::package::PackageName;
use phast_resolver::provider::{PackageMetadata, Provider};
use phast_resolver::resolution::Resolution;
use phast_version::Version;

use chrono::Utc;
use futures::future::join_all;

/// Default Packagist registry URL.
/// Used when no `Packagist` repository entry is present in the manifest.
const DEFAULT_REGISTRY_URL: &str = "https://repo.packagist.org";

/// Routes each resolver call to the source that serves that package.
///
/// A package named in `[sources]` goes straight to the provider pinned there. Every
/// other package takes the fallback chain, the providers no `[sources]` entry claims,
/// tried in declaration order. With no `[sources]` at all the chain holds every
/// provider and the behavior is a plain sequential scan.
#[derive(Clone)]
pub struct ChainedProvider {
    /// Routing table: owns providers, sources, and fallback indices.
    router: RegistryRouter,
}

#[rustfmt::skip]
impl ChainedProvider {
    /// Create a new, empty provider chain with the given HTTP client.
    ///
    /// Call [`with_manifest`](Self::with_manifest) or [`with_url`](Self::with_url)
    /// on the returned value to populate the registry list.
    pub fn new(client: HttpClient, cache_root: PathBuf) -> Self {
        Self {
            router: RegistryRouter::new(client, cache_root),
        }
    }

    /// Populate the chain with a single registry at an explicit URL.
    ///
    /// Used when `--registry-url` is passed on the CLI, bypassing the manifest
    /// repository list entirely. `base` only anchors `path` repositories, which this
    /// builder never adds, and is taken anyway to keep the builder signatures uniform.
    // TODO: take on_warning callback like with_manifest, so repository warnings reach the user.
    pub async fn with_url(
        mut self,
        base: &Path,
        url: &str,
        options: RegistryOptions,
    ) -> anyhow::Result<Self> {
        let providers = vec![
            PackageProvider::Registry {
                url,
                name: None,
            },
        ];

        self.router
            .resolve(base, providers, &HashMap::new(), options, |_| {})
            .await?;

        Ok(self)
    }

    /// Populate the chain from the repositories declared in a manifest.
    ///
    /// Repositories are added in declaration order, then Packagist is appended as the
    /// implicit fallback. A `Packagist` entry in the manifest overrides that: carrying
    /// a `url` it moves the fallback elsewhere, carrying none it removes the fallback
    /// entirely. `path` repositories are resolved relative to `base` (the project root).
    ///
    /// `on_warning` is called for each repository type Phast does not yet support. The
    /// caller decides how to surface those; pass `|_| {}` to ignore them.
    pub async fn with_manifest(
        mut self,
        base: &Path,
        manifest: &Manifest,
        options: RegistryOptions,
        mut on_warning: impl FnMut(&str),
    ) -> anyhow::Result<Self> {
        let mut default_url: Option<&str> = Some(DEFAULT_REGISTRY_URL);
        let mut providers: Vec<PackageProvider> = vec![];

        for repository in &manifest.repositories {
            match repository {
                Repository::Composer { url, name, .. } => {
                    providers.push(PackageProvider::Registry { url, name: name.as_deref() })
                },
                Repository::Packagist { url, .. } => {
                    default_url = url.as_deref()
                },
                Repository::Path { path, .. } => {
                    providers.push(PackageProvider::Path { rel_path: path })
                },
                Repository::Vcs { url, name } => {
                    providers.push(PackageProvider::Vcs { url, name: name.as_deref() })
                },
                _ => on_warning(&format!(
                    "unsupported repository type in manifest, skipping: {:?}",
                    repository,
                )),
            }
        }

        providers.extend(
            default_url.map(|url| {
                PackageProvider::Registry {
                    url,
                    name: None,
                }
            }),
        );

        self.router
            .resolve(base, providers, &manifest.sources, options, on_warning)
            .await?;

        Ok(self)
    }

    /// Build a `PhastLock` from resolved packages using cached metadata.
    ///
    /// For each resolved package, tries `lock` on each routed provider in order and
    /// takes the first success. Registry metadata was already cached during resolution,
    /// so those entries cost nothing, but a git-sourced package still reads its manifest
    /// at the pinned commit from the host.
    pub async fn enrich(
        &self,
        resolution: &Resolution,
        content_hash: &str,
        autoload: AutoloadSection,
        autoload_dev: AutoloadSection,
        mut on_locked: impl FnMut(&str, &str),
    ) -> anyhow::Result<PhastLock> {
        let mut packages = Vec::new();
        let mut packages_dev = Vec::new();

        for (name, version) in &resolution.packages {
            // Routed packages have one provider; unrouted ones fall back to the
            // chain, so trying each and taking the first success is the routing
            // step. The last failure is kept: when every provider declines, that
            // error says why, where a generic message would hide it.
            let mut locked = None;
            let mut failed = None;
            for provider in self.router.route(name) {
                match provider.lock(name, version).await {
                    Ok(pkg) => {
                        locked = Some(pkg);
                        break;
                    },
                    Err(error) => {
                        failed = Some(error);
                    },
                }
            }
            let locked = match (locked, failed) {
                (Some(locked), _) => locked,
                (None, Some(error)) => return Err(error.context(format!(
                    "could not lock {}@{}",
                    name.as_str(),
                    version,
                ))),
                (None, None) => anyhow::bail!(
                    "no provider serves {}@{}",
                    name.as_str(),
                    version,
                ),
            };

            on_locked(locked.name.as_str(), &locked.version);

            if resolution.dev_package_names.contains(name) {
                packages_dev.push(locked);
            } else {
                packages.push(locked);
            }
        }

        let metadata = MetadataSection {
            generated_by: generated_by(),
            generated_at: Utc::now().to_rfc3339(),
        };

        Ok(PhastLock {
            version: CURRENT_VERSION,
            content_hash: content_hash.to_owned(),
            metadata,
            packages: sort::kahn(packages),
            packages_dev: sort::kahn(packages_dev),
            autoload,
            autoload_dev,
        })
    }

    /// Every package a registry in the chain rejected versions of, with the versions and why.
    pub fn rejections(&self) -> Vec<(PackageName, Vec<(Version, Rejection)>)> {
        self.router
            .providers
            .iter()
            .flat_map(|provider| match provider {
                RoutedProvider::Registry(registry) => registry.rejections(),
                _ => vec![],
            })
            .collect()
    }
}

/// Routes all `Provider` calls through the routing table: a sourced package goes directly to
/// its pinned provider, everything else is tried against the fallback chain in declaration order.
#[rustfmt::skip]
impl Provider for ChainedProvider {
    /// Return all known versions of `name` from the first provider that returns a
    /// non-empty list.
    ///
    /// A provider with no versions is passed over, but one that fails stops the lookup with
    /// its error. Moving on would let a later repository answer for a package an earlier
    /// one owns, whenever the earlier one is down or refused, and would report the failure
    /// as an unknown package.
    async fn versions(&self, name: &PackageName) -> anyhow::Result<Vec<Version>> {
        for (position, provider) in self.router.route(name).iter().enumerate() {
            let versions = provider.versions(name).await?;
            if !versions.is_empty() {
                self.router.found(name, position);
                return Ok(versions);
            }
        }
        Ok(vec![])
    }

    /// Return metadata for `name` at `version` from the first provider that succeeds.
    ///
    /// Once `versions` has found the package, its route holds only the provider that
    /// listed it, so no other provider can answer here.
    async fn metadata(
        &self,
        name: &PackageName,
        version: &Version,
    ) -> anyhow::Result<PackageMetadata> {
        let mut last_err = anyhow::anyhow!("no providers configured");
        for provider in self.router.route(name) {
            match provider.metadata(name, version).await {
                Ok(m)  => return Ok(m),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    /// Prefetch metadata for `names` in parallel, routing each name to its designated provider.
    ///
    /// Sourced packages are sent only to their pinned provider; unrouted packages are broadcast
    /// to all fallback providers so each can warm its cache independently.
    async fn prefetch(&self, names: &[PackageName]) {
        let mut sourced: HashMap<usize, Vec<PackageName>> = HashMap::new();
        let mut unrouted: Vec<PackageName> = vec![];

        for name in names {
            match self.router.sources.get(name) {
                Some(&id) => sourced.entry(id).or_default().push(name.clone()),
                None      => unrouted.push(name.clone()),
            }
        }

        join_all(
            sourced
                .iter()
                .map(|(&id, names)| self.router.providers[id].prefetch(names))
                .chain(self.router.fallback.iter().map(|p| p.prefetch(&unrouted)))
        )
        .await;
    }
}
