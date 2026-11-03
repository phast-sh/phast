//! The `RegistryProvider`: implements `Provider` against the Packagist API.
//!
//! ## Request flow
//!
//! 1. On construction the base URL is stored but nothing is fetched yet.
//! 2. The first call to `versions` or `metadata` for a package triggers:
//!    a. Discovery: `GET {base_url}/packages.json` to read `metadata-url` (once per instance).
//!    b. Metadata fetch: `GET {metadata_url}`, then `~dev.json`, each an ETag conditional request.
//!    c. Parse the response (v2 expands the minified delta format; v1 deserializes inline entries).
//!    d. Store result in the in-memory cache.
//! 3. Subsequent calls for the same package are served from the in-memory cache.
//!
//! ## Filtering
//!
//! `versions` returns only what clears two filters. The first is a stability floor,
//! which a root `@stability` flag can lower for one package without moving it for the
//! rest. The second is a minimum publish age, which a version carrying no timestamp,
//! or one that does not parse, is given the benefit of rather than being dropped.

use crate::filter::{Rejection, VersionFilter};
use crate::http::cache::{Cache, MetadataKind};
use crate::http::client::HttpClient;
use crate::p2::P2Version;
use crate::p2::convert::strip_nulls;
use crate::p2::convert::{convert_autoload, convert_dependency_map, convert_to_str_vec};
use crate::p2::parser::ParsedPackage;
use crate::provider::Lockable;
use crate::router::RegistryOptions;

use std::sync::Arc;

use phast_core::lock::phast::PackageDef;
use phast_core::lock::phast::{DistSection, ExtraSection, SourceRef};
use phast_core::package::{PackageName, is_platform};
use phast_core::shared::{Author, Support};
use phast_resolver::provider::{PackageMetadata, Provider};
use phast_version::{Version, VersionSpec};

use anyhow::{Context, bail};
use chrono::DateTime;
use dashmap::DashMap;
use reqwest::{StatusCode, Url, header};
use serde_json::Map;
use serde_json::Value;
use tokio::sync::OnceCell;
use tokio::task::JoinSet;

/// Maximum number of concurrent prefetch requests during resolution.
///
/// Packagist asks for no more than 20 concurrent requests against its static files,
/// which p2 metadata is, so this is a published limit rather than a tuned one.
const PREFETCH_CONCURRENCY: usize = 20;

/// Per-package metadata cell: `None` = confirmed not on registry, `Some` = parsed.
type PackageCell = Arc<OnceCell<Option<Arc<ParsedPackage>>>>;

/// Registry API version, resolved on first use by fetching `packages.json`.
enum ApiVersion {
    /// Satis v1 inline: all packages were preloaded from `packages.json` into `parsed`.
    V1,
    /// Packagist v2: per-package metadata fetched on demand via this URL template.
    V2(String),
}

impl ApiVersion {
    /// Return the per-package metadata URL for v2, or `None` for v1.
    fn metadata_url_for(&self, package: &PackageName) -> Option<String> {
        match self {
            ApiVersion::V1 => None,
            ApiVersion::V2(v) => Some(v.replace("%package%", package.as_str())),
        }
    }
}

/// Packagist-compatible registry provider.
///
/// Implements `Provider` against the Packagist v2 metadata API (`/p2/%package%.json`)
/// and the v1 Satis inline format where all packages are embedded directly in
/// `packages.json`. The API mode is detected automatically on first use.
#[derive(Clone)]
pub struct RegistryProvider {
    /// Base URL of the registry, e.g. `https://packagist.org`.
    base_url: String,
    /// Resolved API mode, populated exactly once on first use by fetching `packages.json`.
    api_version: Arc<OnceCell<ApiVersion>>,
    /// HTTP client with credentials baked in.
    http_client: HttpClient,
    /// Disk cache for metadata responses and downloaded archives.
    cache: Cache,
    /// The rules a version has to pass to be offered, applied in order.
    filters: Vec<VersionFilter>,
    /// In-memory parsed metadata with in-flight deduplication.
    ///
    /// Each entry is an `Arc<OnceCell<Option<Arc<ParsedPackage>>>>`:
    /// - `None`      is a package definitively absent on this registry (cached 404).
    /// - `Some(pkg)` is one successfully fetched and parsed.
    ///
    /// `DashMap::entry()` is shard-locked, so exactly one `OnceCell` is ever created
    /// per package name. `OnceCell::get_or_try_init` then ensures exactly one fetch
    /// runs: the first caller populates the cell while concurrent callers wait on it.
    /// Transient errors are not cached, so the next caller retries.
    parsed: Arc<DashMap<PackageName, PackageCell>>,
    /// Package name → the versions its last lookup rejected, each with the first filter's reason.
    rejections: Arc<DashMap<PackageName, Vec<(Version, Rejection)>>>,
}

#[rustfmt::skip]
impl RegistryProvider {
    /// Create a new provider pointing at `base_url` with the given minimum stability and age.
    ///
    /// The caller supplies the shared HTTP client so that the metadata fetch and
    /// archive download phases reuse the same connection pool.
    pub fn new(base_url: impl Into<String>, client: HttpClient, options: RegistryOptions) -> Self {
        let base_url = base_url.into();
        let host = Url::parse(&base_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default();
        Self {
            base_url,
            api_version: Arc::new(OnceCell::new()),
            http_client: client,
            cache: Cache::new(Cache::default_root()).with_host(host),
            filters: VersionFilter::all(&options),
            parsed: Arc::new(DashMap::new()),
            rejections: Arc::new(DashMap::new()),
        }
    }

    /// Every package a lookup rejected versions of, with the versions and why.
    pub fn rejections(&self) -> Vec<(PackageName, Vec<(Version, Rejection)>)> {
        self.rejections
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }

    /// Replace the cache wholesale, `cache` being used exactly as handed over.
    ///
    /// That includes its host scoping, so a cache built without one drops the
    /// per-registry `metadata/{host}/` split that [`new`](Self::new) sets up.
    pub fn with_cache(mut self, cache: Cache) -> Self {
        self.cache = cache;
        self
    }

    /// Produce a `PackageDef` for the given `name` and `version` from cached metadata.
    ///
    /// Called for every resolved package during `enrich`. All metadata was populated
    /// during resolution, so no network requests are made here.
    async fn locked(&self, name: &PackageName, version: &Version) -> anyhow::Result<PackageDef> {
        let package = self.fetch_parsed(name).await?;
        let (_, ver) = package
            .iter()
            .flat_map(|package| package.versions.iter())
            .find(|(v, _)| v == version)
            .with_context(|| {
                format!(
                    "version {} of '{}' missing from cache during enrichment",
                    version,
                    name.as_str(),
                )
            })?;

        let dist = ver.dist.as_ref().map(|d| DistSection {
            dist_type: d.dist_type.clone(),
            url: d.url.clone(),
            reference: d.reference.clone(),
            shasum: d.shasum.clone(),
        });

        let source = ver.source.as_ref().map(|s| SourceRef {
            vcs_type: s.vcs_type.clone(),
            url: s.url.clone(),
            reference: s.reference.clone(),
        });

        let autoload = convert_autoload(&ver.autoload);
        let license  = convert_to_str_vec(&ver.license);

        let authors = ver
            .authors
            .iter()
            .map(|a| Author {
                name: a.name.clone(),
                email: a.email.clone(),
                homepage: a.homepage.clone(),
                role: a.role.clone(),
            })
            .collect();

        // Store the original (pretty) version the registry reports, e.g.
        // `v2.0.0`, matching composer.lock. The resolved `Version` is a lossy
        // normalization (`2.0`); the pretty string is what dist-URL
        // `%prettyVersion%` placeholders resolve against. Falls back to the
        // normalized form if the registry omits it.
        let version = ver
            .version
            .clone()
            .unwrap_or_else(|| version.to_string());

        let support = Support {
            entries: ver
                .support
                .clone()
                .into_iter()
                .collect(),
        };

        let extra = ver
            .extra
            .clone()
            .and_then(strip_nulls)
            .map(|data| ExtraSection { data });

        Ok(PackageDef {
            name: name.as_str().to_owned(),
            version,
            path: None,
            require: ver.require.clone().into_iter().collect(),
            require_dev: Default::default(),
            dist,
            source,
            package_type: ver.package_type.clone(),
            description: ver.description.clone(),
            homepage: ver.homepage.clone(),
            time: ver.time.clone(),
            notification_url: ver.notification_url.clone(),
            license,
            keywords: ver.keywords.clone(),
            authors,
            support,
            extra,
            autoload,
            provide: ver.provide.clone().into_iter().collect(),
            bin: ver.bin.clone(),
        })
    }

    /// Return the per-package metadata URL for the given package.
    ///
    /// On the first call, fetches `packages.json` to detect whether this registry
    /// speaks v2 (`metadata-url`) or v1 (inline `packages`). All subsequent calls
    /// are served from the cached `api_version` without any network round-trip.
    ///
    /// `OnceCell::get_or_try_init` guarantees that exactly one `packages.json`
    /// request is ever issued per `RegistryProvider` instance, even under concurrent
    /// callers, and that transient failures are not cached.
    async fn discover(&self, package: &PackageName) -> anyhow::Result<Option<String>> {
        let version = self
            .api_version
            .get_or_try_init(|| self.detect_api_version())
            .await?;
        Ok(version.metadata_url_for(package))
    }

    /// Fetch `packages.json` and determine the registry's API version.
    ///
    /// For v1 registries, pre-populates `self.parsed` with all inline packages
    /// so no further per-package requests are needed.
    async fn detect_api_version(&self) -> anyhow::Result<ApiVersion> {
        let packages_url = format!("{}/packages.json", self.base_url);

        let response = self.http_client.get(&packages_url)?
            .send()
            .await
            .with_context(|| format!("could not reach the registry at {}", self.base_url))?
            .error_for_status()
            .with_context(|| format!("registry at {} returned an error for packages.json", self.base_url))?;

        let data = response
            .json::<RegistryResponse>()
            .await
            .context("failed to parse packages.json")?;

        // Prefer v2 (metadata-url) over v1 (inline packages): Composer v2 does the same,
        // and some private registries emit both keys but serve packages
        // on-demand via the metadata URL rather than inline.
        if let Some(template) = data.metadata_url {
            return Ok(ApiVersion::V2(self.to_abs_url(&template)));
        }

        if let Some(inline) = data.packages.as_object().filter(|m| !m.is_empty()) {
            self.populate_v1_cache(inline);
            return Ok(ApiVersion::V1);
        }

        bail!(
            "registry at {} advertises neither 'metadata-url' (v2) nor inline 'packages' (v1)",
            self.base_url,
        )
    }

    /// Pre-populate the in-memory cache from a v1 inline `packages` object.
    ///
    /// `set` is a no-op on an already-initialized `OnceCell`, so calling this
    /// more than once (e.g. from concurrent slow-path callers) is safe.
    fn populate_v1_cache(&self, inline: &Map<String, Value>) {
        for (name, json) in inline {
            // Skip non-package keys (e.g. Satis emits "__root__" for the root package).
            // All valid Composer package names are vendor/package and contain exactly one slash.
            if !name.contains('/') { continue; }
            let name = PackageName::new(name);

            let cell = self
                .parsed
                .entry(name.clone())
                .or_insert_with(|| Arc::new(OnceCell::new()))
                .clone();
            let _ = cell
                .set(ParsedPackage::from_v1(json)
                .map(Arc::new));
        }
    }

    /// Return the parsed metadata for a package, fetching and caching it if needed.
    ///
    /// Calls `discover` to detect the API version on first use. For v1 registries the
    /// package is read directly from the pre-populated cache; for v2 it is fetched on
    /// demand.
    ///
    /// Caching semantics:
    /// - A successful parse (`Some`) is cached permanently: concurrent callers share
    ///   one fetch via `OnceCell`.
    /// - A definitive 404 (`None`) is also cached: the package is absent on this registry
    ///   and retrying would just waste a round-trip.
    /// - Transient errors (network, 5xx, parse failure) are NOT cached: `get_or_try_init`
    ///   leaves the cell uninitialised so the next call retries.
    ///
    /// `None` is a package this registry does not carry, which is an answer, not a failure:
    /// only an `Err` means the registry could not be asked.
    async fn fetch_parsed(&self, name: &PackageName) -> anyhow::Result<Option<Arc<ParsedPackage>>> {
        // Platform packages: php, ext-*, lib-*, composer-*
        // are never on any registry.
        if is_platform(name.as_str()) {
            return Ok(None);
        }

        let package_url = self.discover(name).await?;
        let package: Option<Arc<ParsedPackage>> = match package_url {
            None => {
                // v1: all packages were pre-populated from packages.json during discovery.
                // Use get() here so we don't create an empty cell for absent packages,
                // which would cause prefetch to skip them on later providers.
                return Ok(self
                    .parsed
                    .get(name)
                    .and_then(|cell| cell.get().and_then(|pkg| pkg.as_ref()).cloned()));
            }
            Some(url) => {
                // v2: fetch per-package metadata on demand.
                //
                // `get_or_try_init` only stores the value when the future returns `Ok`.
                // An `Err` (transient failure) leaves the cell uninitialised so the
                // next caller retries, while `Ok(None)` (404) is cached to avoid
                // repeated round-trips for packages that don't exist.
                let cell = self
                    .parsed
                    .entry(name.clone())
                    .or_insert_with(|| Arc::new(OnceCell::new()))
                    .clone();
                cell.get_or_try_init(|| self.fetch_p2(name, url))
                    .await?
                    .clone()
            }
        };

        Ok(package)
    }

    /// Fetch and merge a package's tagged and dev-branch p2 metadata.
    ///
    /// Packagist serves them in separate `{pkg}.json` and `{pkg}~dev.json` files;
    /// either may 404 (a package with only tags, or only dev branches), so the
    /// package is absent only when both are missing.
    async fn fetch_p2(
        &self,
        name: &PackageName,
        url: String,
    ) -> anyhow::Result<Option<Arc<ParsedPackage>>> {
        let mut all_versions = self.fetch_versions(name, &url, MetadataKind::Tagged).await?;
        if let Some(dev_url) = self.to_dev_url(&url) {
            let dev_versions = self.fetch_versions(name, &dev_url, MetadataKind::Dev).await?;
            all_versions.extend(dev_versions);
        }

        if all_versions.is_empty() {
            return Ok(None);
        }

        Ok(Some(Arc::new(ParsedPackage { versions: all_versions })))
    }

    /// Fetch and parse one p2 metadata URL, returning its versions (empty on 404).
    async fn fetch_versions(
        &self,
        name: &PackageName,
        url: &str,
        kind: MetadataKind,
    ) -> anyhow::Result<Vec<(Version, P2Version)>> {
        match self.fetch_raw_json(name, url, kind).await? {
            Some(json) => Ok(ParsedPackage::from_p2_response(name, &json)?.versions),
            None       => Ok(vec![]),
        }
    }

    /// Fetch the raw p2 JSON for a package, using the disk cache with ETag validation.
    ///
    /// - `Ok(Some(json))` is a body from the network (200) or from disk (304 Not Modified).
    /// - `Ok(None)`       is a package definitively absent (404); the caller may cache it.
    /// - `Err(_)`         is a transient failure (network, 5xx); the caller must not cache it.
    async fn fetch_raw_json(
        &self,
        name: &PackageName,
        url: &str,
        kind: MetadataKind,
    ) -> anyhow::Result<Option<String>> {
        let mut request = self.http_client.get(url)?;
        if let Some(et) = self.cache.read_etag(name, kind) {
            request = request.header(header::IF_NONE_MATCH, et);
        }

        let response = request
            .send()
            .await?;

        let status = response.status();
        // Cached response is still valid; serve the body from disk without re-downloading.
        if status == StatusCode::NOT_MODIFIED {
            return self
                .cache
                .read_metadata(name, kind)
                .map(Some)
                .with_context(|| format!("cache file missing after 304 for '{}'", name.as_str()));
        }

        // Package is definitively absent on this registry; caller may cache this.
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        // Any other non-2xx is a transient or auth failure; propagate so caller can retry.
        if !status.is_success() {
            bail!("HTTP {} fetching metadata for '{}' from {}", status, name.as_str(), self.base_url);
        }

        let etag = response.headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim().to_owned());
        let body = response.text().await?;

        self.cache.write_metadata(name, kind, &body, etag.as_deref());
        Ok(Some(body))
    }

    /// Resolve a registry-relative template URL to an absolute URL.
    fn to_abs_url(&self, template: &str) -> String {
        if Url::parse(template).is_ok() {
            return template.to_owned();
        }

        format!("{}{}", self.base_url, template)
    }

    /// `.../vendor/pkg.json` -> `.../vendor/pkg~dev.json`, where dev branches live.
    fn to_dev_url(&self, url: &str) -> Option<String> {
        url.strip_suffix(".json")
            .map(|base| format!("{base}~dev.json"))
    }
}

impl Provider for RegistryProvider {
    /// Return all versions of `name` that every filter passes, newest first.
    async fn versions(&self, name: &PackageName) -> anyhow::Result<Vec<Version>> {
        let Some(package) = self.fetch_parsed(name).await? else {
            return Ok(vec![]);
        };

        let mut versions = Vec::new();
        let mut rejections = Vec::new();

        for (version, p2) in &package.versions {
            // A missing or unparseable timestamp becomes `None`, which the age filter passes.
            let published = p2
                .time
                .as_deref()
                .and_then(|time| DateTime::parse_from_rfc3339(time).ok());

            match self
                .filters
                .iter()
                .try_for_each(|filter| filter.check(name, version, published))
            {
                Ok(()) => versions.push(version.clone()),
                Err(r) => rejections.push((version.clone(), r)),
            }
        }

        // Replaced rather than appended: the resolver asks about a package more than once.
        if rejections.is_empty() {
            self.rejections.remove(name);
        } else {
            self.rejections.insert(name.clone(), rejections);
        }

        // Newest first (descending order).
        versions.sort_unstable_by(|a, b| b.cmp(a));
        Ok(versions)
    }

    /// Return the dependency metadata for a specific version of `name`.
    async fn metadata(
        &self,
        name: &PackageName,
        version: &Version,
    ) -> anyhow::Result<PackageMetadata> {
        let package = self.fetch_parsed(name).await?;
        let (_, pkg) = package
            .iter()
            .flat_map(|package| package.versions.iter())
            .find(|(v, _)| v == version)
            .with_context(|| {
                format!(
                    "version {} of '{}' not found on {}",
                    version,
                    name.as_str(),
                    self.base_url,
                )
            })?;

        let resolve = |v: &str| {
            v.parse::<VersionSpec>()
                .ok()
                .map(|spec| spec.resolve(version))
        };

        let replace: Vec<_> = pkg
            .replace
            .iter()
            .filter_map(|(k, v)| resolve(v).map(|ver| (PackageName::new(k), ver)))
            .collect();

        let provide: Vec<_> = pkg
            .provide
            .iter()
            .filter_map(|(k, v)| resolve(v).map(|ver| (PackageName::new(k), ver)))
            .collect();

        Ok(PackageMetadata {
            name: name.clone(),
            version: version.clone(),
            require: convert_dependency_map(&pkg.require)?,
            require_dev: convert_dependency_map(&pkg.require_dev)?,
            provide,
            replace,
        })
    }

    /// Speculatively fetch metadata for `names` in the background.
    ///
    /// Warms the in-memory cache so the solver stalls less often, skipping any name
    /// already fetched or already confirmed missing.
    ///
    /// A new task is only spawned once a slot opens, so the live task count never
    /// exceeds `PREFETCH_CONCURRENCY` however many dependencies a package declares.
    /// Errors are swallowed: a prefetch that fails costs the solver a stall, not a run.
    async fn prefetch(&self, names: &[PackageName]) {
        let mut tasks: JoinSet<()> = JoinSet::new();

        for name in names {
            // Skip packages already seen (fetched or confirmed missing).
            if self.parsed.contains_key(name) {
                continue;
            }

            // Wait for a slot before spawning to bound the number of live tasks.
            if tasks.len() >= PREFETCH_CONCURRENCY {
                tasks.join_next().await;
            }

            let provider = self.clone();
            let name = name.clone();
            tasks.spawn(async move {
                // Ignore errors - prefetch is best-effort.
                let _ = provider.fetch_parsed(&name).await;
            });
        }

        // Drive remaining tasks to completion before returning.
        while tasks.join_next().await.is_some() {}
    }
}

/// The only leaf serving many packages, so `name` selects one rather than being
/// verified against the provider's own.
impl Lockable for RegistryProvider {
    async fn lock(&self, name: &PackageName, version: &Version) -> anyhow::Result<PackageDef> {
        self.locked(name, version).await
    }
}

/// Deserialized `packages.json` response from a Packagist-compatible registry.
#[derive(serde::Deserialize)]
struct RegistryResponse {
    /// v2: per-package metadata URL template, e.g. `/p2/%package%.json`.
    #[serde(rename = "metadata-url")]
    metadata_url: Option<String>,
    /// v1: inline package data. Kept as `Value` so both `{}` and `[]` (sent by some
    /// registries when empty) deserialize without error. Callers use `.as_object()`.
    #[serde(default)]
    packages: Value,
}
