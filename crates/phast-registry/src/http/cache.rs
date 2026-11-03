//! Disk cache for package metadata and downloaded archives.
//!
//! ## Metadata
//!
//! The raw p2 responses, one file per package per [`MetadataKind`], with an ETag
//! sidecar beside each body. A conditional `If-None-Match` request turns an unchanged
//! resource into a 304 and a disk read. Scoping the cache to a host puts those files
//! under that host's own directory, so an unauthenticated Packagist answer cannot
//! overwrite a private registry's reply for the same package.
//!
//! ## Archives
//!
//! Downloaded zips, keyed by [`dist_key`], which prefers a content address so that one
//! copy serves every project on the machine resolving that package version. A key names
//! fixed content, so an entry is never refreshed in place: a new release is a new key
//! beside the old one. Failing a checksum is the one thing that removes an entry, so
//! the next run fetches a clean copy.

use crate::p2::convert::split_package_name;

use std::fmt;
use std::path::PathBuf;

use phast_core::lock::phast::DistSection;
use phast_core::package::PackageName;

use sha2::{Digest, Sha256};

/// Disk cache for package metadata and downloaded archives.
///
/// All paths are derived from a single `root` directory so the entire cache
/// can be relocated or cleared by changing or removing that directory.
#[derive(Clone)]
pub struct Cache {
    /// Root directory for all cached data.
    root: PathBuf,
    /// Registry hostname used as a subdirectory under `metadata/` to prevent
    /// cross-registry cache poisoning (e.g. unauthenticated Packagist response
    /// overwriting authenticated private registry metadata for the same package).
    host: String,
}

impl Cache {
    /// A cache rooted at `root` and scoped to no registry, so metadata lands
    /// directly under `{root}/metadata/` until [`with_host`](Self::with_host)
    /// narrows it.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            host: String::new(),
        }
    }

    /// Scope this cache to a specific registry host.
    ///
    /// Metadata is stored under `{root}/metadata/{host}/` so each registry
    /// maintains an isolated namespace.
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Resolve the platform-appropriate cache root.
    ///
    /// Linux: `~/.cache/phast`
    /// macOS: `~/Library/Caches/phast`
    /// Windows: `%LOCALAPPDATA%\phast`
    ///
    /// A platform that reports no cache directory gets `.phast-cache/phast`,
    /// which is relative, so the cache lands wherever the command was run.
    pub fn default_root() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from(".phast-cache"))
            .join("phast")
    }

    /// Returns `true` if the cache root directory exists on disk.
    pub fn exists(&self) -> bool {
        self.root.exists()
    }

    /// Directory where p2 metadata responses and ETag sidecars are stored.
    ///
    /// `{root}/metadata/{host}/` when the cache is scoped to a host,
    /// `{root}/metadata/` when it is not.
    pub fn metadata_dir(&self) -> PathBuf {
        if self.host.is_empty() {
            self.root.join("metadata")
        } else {
            self.root.join("metadata").join(&self.host)
        }
    }

    /// Directory where downloaded package archives are stored.
    pub fn archives_dir(&self) -> PathBuf {
        self.root.join("archives")
    }

    /// Path for the cached p2 JSON response for a package's `kind` resource.
    ///
    /// Layout: `{vendor}/{package}.json` for tagged releases, `{vendor}/{package}~dev.json`
    /// for dev, both under [`metadata_dir`](Self::metadata_dir) and so carrying the host
    /// segment whenever one is set.
    pub fn metadata_path(&self, name: &PackageName, kind: MetadataKind) -> PathBuf {
        let (vendor, package) = split_package_name(name);
        self.metadata_dir()
            .join(vendor)
            .join(format!("{}{}.json", package, kind.infix()))
    }

    /// Path for the ETag sidecar file for a package's `kind` metadata.
    ///
    /// Sits beside the body it validates, with `.etag` in place of `.json`.
    pub fn etag_path(&self, name: &PackageName, kind: MetadataKind) -> PathBuf {
        let (vendor, package) = split_package_name(name);
        self.metadata_dir()
            .join(vendor)
            .join(format!("{}{}.etag", package, kind.infix()))
    }

    /// Path for a cached zip archive, keyed by a stable identifier.
    ///
    /// Layout: `{root}/archives/{key}.zip`, with `key` as [`dist_key`] derives it.
    pub fn archive_path(&self, key: &str) -> PathBuf {
        self.archives_dir().join(format!("{}.zip", key))
    }

    /// Read the cached p2 JSON for a package's `kind` resource, if present.
    ///
    /// Returns `None` when no cache entry exists yet.
    pub fn read_metadata(&self, name: &PackageName, kind: MetadataKind) -> Option<String> {
        std::fs::read_to_string(self.metadata_path(name, kind)).ok()
    }

    /// Read the cached ETag for a package's `kind` resource, if present.
    pub fn read_etag(&self, name: &PackageName, kind: MetadataKind) -> Option<String> {
        std::fs::read_to_string(self.etag_path(name, kind)).ok()
    }

    /// Persist a `kind` metadata response body and its ETag to the disk cache.
    ///
    /// Errors are intentionally ignored: a failed cache write is not fatal.
    /// If the registry did not send an ETag, the sidecar file is not written.
    pub fn write_metadata(
        &self,
        name: &PackageName,
        kind: MetadataKind,
        body: &str,
        etag: Option<&str>,
    ) {
        let cache_path = self.metadata_path(name, kind);
        let etag_path = self.etag_path(name, kind);

        if let Some(parent) = cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let _ = std::fs::write(&cache_path, body);

        if let Some(etag) = etag {
            let _ = std::fs::write(&etag_path, etag);
        }
    }
}

impl fmt::Display for Cache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.root.display())
    }
}

/// Derive a stable cache key from a dist entry.
///
/// Priority: `shasum` (content-addressable) > `reference` (commit hash) > URL hash.
/// The URL hash is a last resort for v1 Satis registries that omit both fields.
#[rustfmt::skip]
pub fn dist_key(dist: &DistSection) -> Option<String> {
    dist.shasum
        .as_deref()
        .filter(|s| !s.is_empty())
        .or_else(|| dist.reference.as_deref().filter(|s| !s.is_empty()))
        .map(str::to_owned)
        .or_else(|| {
            if dist.url.is_empty() { return None }

            let digest = Sha256::digest(dist.url.as_bytes());
            Some(format!("url-{}", hex::encode(&digest[..16])))
        })
}

/// Which p2 metadata resource a cache entry belongs to.
///
/// Packagist serves a package's tagged releases and its dev branches as two
/// separate resources (`{pkg}.json` and `{pkg}~dev.json`), each with its own
/// ETag. They must be cached side by side: sharing one slot makes the two
/// clobber each other's body and ETag, so conditional requests never hit 304.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataKind {
    /// `{pkg}.json`, tagged releases.
    Tagged,
    /// `{pkg}~dev.json`, dev branches.
    Dev,
}

impl MetadataKind {
    /// The filename infix that distinguishes the two resources on disk,
    /// mirroring the `~dev` suffix Packagist uses in the URL.
    fn infix(self) -> &'static str {
        match self {
            MetadataKind::Tagged => "",
            MetadataKind::Dev => "~dev",
        }
    }
}
