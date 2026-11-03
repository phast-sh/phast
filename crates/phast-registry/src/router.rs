//! `RegistryRouter`: routing table built from `[[repositories]]` and `[sources]`.
//!
//! ## Format
//!
//! ```toml
//! [[repositories]]
//! name = "nova"
//! type = "composer"
//! url  = "https://nova.laravel.com"
//!
//! [[repositories]]
//! name = "setasign"
//! type = "composer"
//! url  = "https://www.setasign.com/downloads/"
//!
//! [sources]
//! "laravel/nova"  = { repository = "nova" }
//! "setasign/fpdi" = { repository = "setasign" }
//! ```
//!
//! [`RegistryRouter`] would route `laravel/nova` directly to `https://nova.laravel.com`
//! and `setasign/fpdi` to `https://www.setasign.com/downloads/`, leaving all other
//! packages to fall through to the Packagist fallback chain.

use crate::hints;
use crate::http::cache::Cache;
use crate::http::client::HttpClient;
use crate::provider::RegistryProvider;
use crate::provider::{InlineProvider, Lockable, PathProvider, VcsProvider};
use crate::vcs::host_for;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::slice::from_ref;
use std::sync::Arc;
use std::time::Duration;

use phast_core::lock::phast::PackageDef;
use phast_core::manifest::{GitRef, Manifest, Source};
use phast_core::package::PackageName;
use phast_resolver::provider::{PackageMetadata, Provider};
use phast_version::{Stability, Version};

use dashmap::DashMap;

/// A provider the router can route to, whatever kind of source backs it.
///
/// The router holds these uniformly so [`ChainedProvider`](crate::provider::ChainedProvider)
/// never has to know how a package is fetched. Delegates every call to the inner provider.
///
/// Every variant is boxed so the enum stays pointer-sized regardless of the
/// inner provider's footprint (a `PathProvider` carries a full `Manifest`). The
/// router holds only a handful of providers per run and every access is gated by
/// network or cache I/O, so the indirection is immaterial.
#[derive(Clone)]
pub enum RoutedProvider {
    /// A Packagist-compatible registry queried over HTTP.
    Registry(Box<RegistryProvider>),
    /// A package resolved from a local `path` repository.
    Path(Box<PathProvider>),
    /// A package resolved from a `vcs` repository on a git host.
    Vcs(Box<VcsProvider>),
    /// A package resolved from an inline `[sources]` definition.
    Inline(Box<InlineProvider>),
}

#[rustfmt::skip]
impl Provider for RoutedProvider {
    async fn versions(&self, name: &PackageName) -> anyhow::Result<Vec<Version>> {
        match self {
            RoutedProvider::Registry(p) => p.versions(name).await,
            RoutedProvider::Path(p)     => p.versions(name).await,
            RoutedProvider::Vcs(p)      => p.versions(name).await,
            RoutedProvider::Inline(p)   => p.versions(name).await,
        }
    }

    async fn metadata(&self, name: &PackageName, version: &Version) -> anyhow::Result<PackageMetadata> {
        match self {
            RoutedProvider::Registry(p) => p.metadata(name, version).await,
            RoutedProvider::Path(p)     => p.metadata(name, version).await,
            RoutedProvider::Vcs(p)      => p.metadata(name, version).await,
            RoutedProvider::Inline(p)   => p.metadata(name, version).await,
        }
    }

    async fn prefetch(&self, names: &[PackageName]) {
        match self {
            RoutedProvider::Registry(p) => p.prefetch(names).await,
            // Path, VCS, and inline packages have nothing to warm: a path is on
            // disk, and a git ref/inline archive is fetched only at install time.
            RoutedProvider::Path(_) | RoutedProvider::Vcs(_) | RoutedProvider::Inline(_) => {}
        }
    }
}

#[rustfmt::skip]
impl Lockable for RoutedProvider {
    /// Produce the locked package entry, delegating to the inner provider.
    async fn lock(&self, name: &PackageName, version: &Version) -> anyhow::Result<PackageDef> {
        match self {
            RoutedProvider::Registry(p) => p.lock(name, version).await,
            RoutedProvider::Path(p)     => p.lock(name, version).await,
            RoutedProvider::Vcs(p)      => p.lock(name, version).await,
            RoutedProvider::Inline(p)   => p.lock(name, version).await,
        }
    }
}

/// A repository to register in the routing table, classified from the manifest.
#[rustfmt::skip]
pub enum PackageProvider<'a> {
    /// A Packagist-compatible registry at `url`, optionally named for `[sources]`.
    Registry {
        url: &'a str,
        name: Option<&'a str>,
    },
    /// A local `path` repository rooted at `rel_path` (relative to the project root).
    Path {
        rel_path: &'a str,
    },
    /// A `vcs` repository at `url` (on a git host), optionally named for `[sources]`.
    Vcs {
        url: &'a str,
        name: Option<&'a str>,
    },
}

/// Registry-level configuration derived from `[options]`.
///
/// Handed to every provider built from the manifest, so one set of filters governs
/// the whole chain rather than each source deciding for itself.
#[derive(Debug, Clone, Default)]
pub struct RegistryOptions {
    /// Minimum stability a version must have to be returned by `versions()`.
    pub minimum_stability: Stability,
    /// Minimum age a version must have to be returned by `versions()`.
    pub minimum_package_age: Option<Duration>,
    /// Per-package stability floors from root `@stability` flags (`^2.0@dev`),
    /// overriding `minimum_stability` for the named package only.
    pub stability_overrides: HashMap<PackageName, Stability>,
}

#[rustfmt::skip]
impl RegistryOptions {
    /// Derive the registry filters from a project manifest: the global stability
    /// and age floors from `[config]`, plus the per-package stability floors that
    /// `require` / `require-dev` entries ask for, written (`^2.0@dev`) or implied
    /// (`dev-master`).
    pub fn from_manifest(manifest: &Manifest) -> Self {
        let minimum_stability   = manifest.config.minimum_stability;
        let stability_overrides = manifest
            .require
            .iter()
            .chain(manifest.require_dev.iter())
            .filter_map(|(name, spec)| {
                let floor = spec.stability()?.floor(minimum_stability)?;
                Some((name.clone(), floor))
            })
            .collect();

        Self {
            minimum_stability:   manifest.config.minimum_stability,
            minimum_package_age: manifest.config.minimum_package_age,
            stability_overrides,
        }
    }
}

/// Routing table for [`ChainedProvider`](crate::provider::ChainedProvider).
///
/// Owns the HTTP client, the list of providers, and the two routing collections
/// built from the manifest. `route` returns the single provider for packages listed in
/// `[sources]` or found in a provider, or the ordered fallback slice for everything else.
/// Every case returns a `&[RoutedProvider]` with no per-call allocation.
#[derive(Clone)]
pub struct RegistryRouter {
    /// HTTP client shared by all providers in this router.
    http_client: HttpClient,
    /// Cache root used for all providers; defaults to `Cache::default_root()`.
    cache_root: PathBuf,
    /// All configured providers in declaration order (manifest repos first, Packagist last).
    pub providers: Vec<RoutedProvider>,
    /// Package name → index into `providers` for explicitly sourced packages.
    pub sources: HashMap<PackageName, usize>,
    /// Package name → index into `fallback` for packages found in a provider.
    pub origins: Arc<DashMap<PackageName, usize>>,
    /// Pre-materialized fallback providers (cloned from `providers`, sharing Arc state),
    /// returned directly by `route` for unrouted packages with no per-call allocation.
    pub fallback: Vec<RoutedProvider>,
}

#[rustfmt::skip]
impl RegistryRouter {
    /// Create an empty router with the given HTTP client and cache root.
    pub fn new(http_client: HttpClient, cache_root: PathBuf) -> Self {
        Self {
            http_client,
            cache_root,
            providers: vec![],
            sources: HashMap::new(),
            origins: Arc::new(DashMap::new()),
            fallback: vec![],
        }
    }

    /// Return the providers to query for `name`: one for sourced or found packages,
    /// the fallback slice otherwise.
    pub fn route(&self, name: &PackageName) -> &[RoutedProvider] {
        if let Some(&id) = self.sources.get(name) {
            return from_ref(&self.providers[id]);
        }

        match self.origins.get(name).map(|id| *id) {
          Some(id) => from_ref(&self.fallback[id]),
          None     => &self.fallback,
      }
    }

    /// Record that `name` was found in the provider at `position` of its route.
    ///
    /// The first record stands: a found package's route holds one provider, at position 0,
    /// and a repeated lookup must not overwrite the real position with that one.
    pub fn found(&self, name: &PackageName, position: usize) {
        if self.sources.contains_key(name) {
            return;
        }

        self.origins.entry(name.clone()).or_insert(position);
    }

    /// Build providers from `repositories`, resolve package-to-provider routing, and
    /// compute the fallback list.
    ///
    /// `base` is the project root, used to resolve relative `path` repositories.
    /// Fallible because registering a source reads from it: a path repository's
    /// manifest from disk, a git one's from its host.
    pub async fn resolve(
        &mut self,
        base: &Path,
        repositories: Vec<PackageProvider<'_>>,
        sources: &HashMap<PackageName, Source>,
        options: RegistryOptions,
        on_warning: impl FnMut(&str),
    ) -> anyhow::Result<()> {
        self.build_routing(base, repositories, sources, options, on_warning)
            .await?;

        let explicit: HashSet<usize> = self.sources
            .values()
            .copied()
            .collect();

        self.fallback = (0..self.providers.len())
            .filter(|s| !explicit.contains(s))
            .map(|i| self.providers[i].clone())
            .collect();

        Ok(())
    }

    /// Register a provider per repository and resolve the routing table.
    ///
    /// Registry repositories are pinned by `[sources]` aliases; path and VCS
    /// repositories self-pin by the package name they declare. A VCS repository
    /// on an unsupported host is reported via `on_warning` and skipped.
    ///
    /// Repositories are registered before `[sources]` is walked, and the two settle
    /// a clash differently by source kind. A `{ repository }` entry yields to a self-pin,
    /// since a local or forked package is the more explicit intent.
    /// A `{ path }`, `{ url }` or `{ package }` entry replaces it, being itself the more
    /// specific statement about that one package.
    async fn build_routing(
        &mut self,
        base: &Path,
        repositories: Vec<PackageProvider<'_>>,
        sources: &HashMap<PackageName, Source>,
        options: RegistryOptions,
        mut on_warning: impl FnMut(&str),
    ) -> anyhow::Result<()> {
        let mut registry: HashMap<&str, usize> = HashMap::new();

        // Build a provider per [[repositories]] endpoint:
        //  - a registry records its name for [sources] to reference
        //  - a path/vcs repo self-pins by the name it declares.
        for repository in repositories {
            match repository {
                PackageProvider::Registry { url, name } => {
                    let provider = RegistryProvider::new(url, self.http_client.clone(), options.clone())
                        .with_cache(Cache::new(&self.cache_root));
                    let index = self
                        .add_provider(RoutedProvider::Registry(Box::new(provider)));
                    if let Some(name) = name {
                        registry.insert(name, index);
                    }
                }
                PackageProvider::Path { rel_path } => {
                    self.pin_path(base, rel_path, None)?
                },
                PackageProvider::Vcs { url, name } => {
                    let index = self
                        .pin_vcs(url, options.clone(), &mut on_warning)
                        .await?;
                    if let (Some(name), Some(index)) = (name, index) {
                        registry.insert(name, index);
                    }
                },
            }
        }

        // Route each [sources] package to its provider:
        //  - `{ repository }` to a named registry
        //  - `{ path }` to its own path provider
        //  - `{ url }` to a git pin at the ref it names.
        for (name, source) in sources {
            match source {
                Source::Repository { repository } => {
                    if let Some(&index) = registry.get(repository.as_str()) {
                        self.sources.entry(name.clone()).or_insert(index);
                    }
                }
                Source::Path { path, version, .. } => {
                    self.pin_path(base, path, version.as_deref())?
                },
                Source::Package { package } => {
                    self.pin_inline(package, &mut on_warning)?
                },
                Source::Git { url, reference } => {
                    self.pin_git(name, url, reference, &mut on_warning)
                        .await?
                },
            }
        }

        Ok(())
    }

    /// Push a provider and return its index in `providers`.
    fn add_provider(&mut self, provider: RoutedProvider) -> usize {
        let index = self.providers.len();
        self.providers.push(provider);
        index
    }

    /// Load a `path` package and pin it by the name it declares. `version` is the
    /// `[sources]` pin, if any (see [`PathProvider::load`]).
    fn pin_path(&mut self, base: &Path, rel_path: &str, version: Option<&str>) -> anyhow::Result<()> {
        let provider = PathProvider::load(base, rel_path, version)?;

        let name  = provider.name().clone();
        let index = self.add_provider(RoutedProvider::Path(Box::new(provider)));
        self.sources.insert(name, index);

        Ok(())
    }

    /// Discover a `vcs` package and pin it by the name it declares, returning the
    /// provider's index so the caller can also register the repository's logical
    /// name. An unsupported host is reported via `on_warning` and skipped (`None`).
    async fn pin_vcs(
        &mut self,
        url: &str,
        options: RegistryOptions,
        on_warning: &mut impl FnMut(&str),
    ) -> anyhow::Result<Option<usize>> {
        let Some(host) = host_for(url, self.http_client.clone()) else {
            on_warning(&hints::unsupported_vcs_host(url));
            return Ok(None);
        };

        let provider = VcsProvider::discover(url, host, options)
            .await?;

        let name  = provider.name().clone();
        let index = self.add_provider(RoutedProvider::Vcs(Box::new(provider)));
        self.sources.insert(name, index);

        Ok(Some(index))
    }

    /// Pin `name` to the single-package git source at `url`, at the ref it names.
    ///
    /// Unlike a `vcs` `[[repositories]]` entry, which discovers every ref and lets
    /// the resolver choose, the pin offers exactly one candidate. The package name
    /// comes from the `[sources]` key, so `VcsProvider::pin` verifies the repository
    /// agrees with it. An unsupported host is reported via `on_warning` and skipped.
    async fn pin_git(
        &mut self,
        name: &PackageName,
        url: &str,
        reference: &GitRef,
        on_warning: &mut impl FnMut(&str),
    ) -> anyhow::Result<()> {
        let Some(host) = host_for(url, self.http_client.clone()) else {
            on_warning(&hints::unsupported_vcs_host(url));
            return Ok(());
        };

        let provider = VcsProvider::pin(url, host, reference, name)
            .await?;

        let index = self.add_provider(RoutedProvider::Vcs(Box::new(provider)));
        self.sources.insert(name.clone(), index);

        Ok(())
    }

    /// Build an inline-package provider and pin it by its declared name. A source
    /// host Phast cannot derive an archive from is reported via `on_warning` and
    /// skipped (the same unsupported-host handling as a `vcs` source).
    fn pin_inline(
        &mut self,
        package: &serde_json::Value,
        on_warning: &mut impl FnMut(&str),
    ) -> anyhow::Result<()> {
        let url = package
            .get("source")
            .and_then(|s| s.get("url"))
            .and_then(|v| v.as_str());

        let Some(host) = url
            .and_then(|url| host_for(url, self.http_client.clone())) else
        {
            let name = package
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            on_warning(&hints::unsupported_inline_host(name));
            return Ok(());
        };

        let provider = InlineProvider::build(package, host)?;

        let name  = provider.name().clone();
        let index = self.add_provider(RoutedProvider::Inline(Box::new(provider)));
        self.sources.insert(name, index);

        Ok(())
    }
}
