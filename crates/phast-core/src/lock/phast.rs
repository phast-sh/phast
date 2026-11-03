//! `phast.lock` serialization and deserialization.
//!
//! phast.lock is the canonical TOML lock file. What it records is what an install needs to
//! reproduce a `vendor/` without resolving: the chosen version of every package, where to
//! fetch it, and the autoload rules to generate from it. The conditions that produced those
//! choices are not stored, they reach the file only through `content-hash`.
//!
//! ## Format
//!
//! The example below shows every key a `phast.lock` can carry. Most per-package
//! fields are optional and omitted when empty; a real lock is usually far shorter.
//!
//! ```toml
//! version = 1
//! content-hash = "sha256:abc123..."
//!
//! [metadata]
//! generated-by = "phast 0.1.0"
//! generated-at = "2026-04-08T12:00:00Z"
//!
//! # A registry package with the full set of metadata fields.
//! [[packages]]
//! name             = "acme/framework"
//! version          = "v11.0.0"
//! type             = "library"
//! description      = "The Acme Framework"
//! homepage         = "https://acme.example"
//! time             = "2026-03-12T14:22:00Z"
//! notification-url = "https://packages.acme.example/downloads/"
//! license          = ["MIT"]
//! keywords         = ["framework", "acme"]
//! bin              = ["bin/console"]
//!
//! [packages.require]
//! php        = ">=8.2"
//! "acme/log" = "^3.0"
//!
//! [packages.provide]
//! "acme/log-implementation" = "3.0"
//!
//! [packages.dist]
//! type      = "zip"
//! url       = "https://api.github.com/repos/acme/framework/zipball/abc123"
//! reference = "abc123"
//! shasum    = "deadbeef"
//!
//! [packages.source]
//! type      = "git"
//! url       = "https://github.com/acme/framework.git"
//! reference = "abc123"
//!
//! [packages.support]
//! issues = "https://github.com/acme/framework/issues"
//! source = "https://github.com/acme/framework"
//!
//! [packages.extra]
//! branch-alias = { "dev-main" = "11.x-dev" }
//!
//! # Autoload: array values (classmap, files) precede the psr sub-tables.
//! [packages.autoload]
//! classmap = ["database/"]
//! files    = ["src/helpers.php"]
//!
//! [packages.autoload.psr-4]
//! "Acme\\Framework\\" = ["src/"]
//!
//! [packages.autoload.psr-0]
//! "Legacy_" = ["lib/legacy/"]
//!
//! [[packages.authors]]
//! name  = "Acme Engineering"
//! email = "dev@acme.example"
//!
//! # A workspace member: identified by `path`, carries no dist/source, and keeps
//! # its dev edges separable via `require-dev`.
//! [[packages]]
//! name    = "acme/logger"
//! version = "1.4.0"
//! path    = "libs/logger"
//!
//! [packages.require]
//! "acme/log" = "^3.0"
//!
//! [packages.require-dev]
//! "acme/test-tools" = "^11.0"
//!
//! [packages.autoload.psr-4]
//! "Acme\\Logger\\" = ["src/"]
//!
//! [[packages-dev]]
//! name    = "acme/test-tools"
//! version = "11.0.0"
//!
//! [packages-dev.dist]
//! type      = "zip"
//! url       = "https://api.github.com/repos/acme/test-tools/zipball/def456"
//! reference = "def456"
//! shasum    = "cafed00d"
//!
//! # Root package autoload rules.
//! [autoload.psr-4]
//! "App\\" = ["app/"]
//!
//! [autoload-dev.psr-4]
//! "Tests\\" = ["tests/"]
//! ```

use crate::manifest::{Autoload, VcsType};
use crate::shared::{Author, Support};

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Lock file format version written into every `phast.lock`.
/// Bumped only on breaking schema changes.
pub const CURRENT_VERSION: u32 = 1;

/// The root of a `phast.lock` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhastLock {
    /// Lock file format version. Bumped only on breaking changes.
    pub version: u32,

    /// sha256 hash of the manifest inputs that produced this lock.
    /// Used to detect when the lock is stale without re-resolving.
    #[serde(rename = "content-hash")]
    pub content_hash: String,

    /// Informational metadata written at lock generation time (tool name, timestamp).
    pub metadata: MetadataSection,

    /// Production packages.
    #[serde(default)]
    pub packages: Vec<PackageDef>,

    /// Dev-only packages.
    #[serde(rename = "packages-dev", default)]
    pub packages_dev: Vec<PackageDef>,

    /// Root package autoload rules.
    #[serde(default, skip_serializing_if = "AutoloadSection::is_empty")]
    pub autoload: AutoloadSection,

    /// Root package dev-only autoload rules.
    #[serde(
        rename = "autoload-dev",
        default,
        skip_serializing_if = "AutoloadSection::is_empty"
    )]
    pub autoload_dev: AutoloadSection,
}

impl PhastLock {
    /// Read a `phast.lock` from a file path.
    pub fn read(path: &Path) -> Result<Self, PhastLockError> {
        let content = std::fs::read_to_string(path)?;
        let lock: Self = toml::from_str(&content)?;

        if lock.version != CURRENT_VERSION {
            return Err(PhastLockError::UnsupportedVersion {
                found: lock.version,
                expected: CURRENT_VERSION,
            });
        }

        Ok(lock)
    }

    /// Write this `phast.lock` to a file path.
    ///
    /// Writes to a `.tmp` sibling first, then renames -- atomic on POSIX, so a
    /// crash mid-write never produces a corrupt lock.
    pub fn write(&self, path: &Path) -> Result<(), PhastLockError> {
        let content = toml::to_string_pretty(self)?;

        let tmp = path.with_extension("lock.tmp");
        std::fs::write(&tmp, content)?;
        std::fs::rename(&tmp, path)?;

        Ok(())
    }

    /// Check whether this lock is still valid for the given content hash.
    pub fn is_fresh(&self, content_hash: &str) -> bool {
        self.content_hash == content_hash
    }
}

/// Outcome of attempting to load an existing lock file.
#[derive(Debug, Clone)]
pub enum PhastLockState {
    /// Lock exists and is consistent with the current manifest.
    Fresh(PhastLock),
    /// Lock exists but is out of date; the stale lock is kept for cache warming.
    Stale(PhastLock),
    /// No lock file found.
    Missing,
}

/// Errors that can occur when reading or writing `phast.lock`.
#[derive(Debug, Error)]
pub enum PhastLockError {
    #[error("failed to parse phast.lock: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("failed to serialize phast.lock: {0}")]
    Serialize(#[from] toml::ser::Error),

    #[error("I/O error reading/writing phast.lock: {0}")]
    Io(#[from] std::io::Error),

    #[error("unsupported phast.lock version {found}, expected {expected}")]
    UnsupportedVersion { found: u32, expected: u32 },
}

/// Informational metadata written into `phast.lock` at generation time.
///
/// Not used during resolution or installation -- purely for manual inspection
/// and debugging (e.g. knowing which version of Phast produced a given lock).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetadataSection {
    /// Name and version of the tool that generated the lock, e.g. `"phast 0.1.0"`.
    #[serde(rename = "generated-by")]
    pub generated_by: String,

    /// ISO 8601 timestamp of when the lock was generated.
    #[serde(rename = "generated-at")]
    pub generated_at: String,
}

/// Runtime dependencies of a locked package (`[packages.require]` /
/// `[packages.require-dev]`): package name -> resolved constraint.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequireSection {
    /// Package name -> constraint. Flattened, so entries are the table's keys.
    /// Sorted, so the table does not reorder between writes.
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub entries: BTreeMap<String, String>,
}

impl RequireSection {
    /// Returns `true` when the package declares no dependencies.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl From<BTreeMap<String, String>> for RequireSection {
    fn from(entries: BTreeMap<String, String>) -> Self {
        Self { entries }
    }
}

impl FromIterator<(String, String)> for RequireSection {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(iter: I) -> Self {
        Self {
            entries: iter.into_iter().collect(),
        }
    }
}

/// Virtual packages a locked package provides (`[packages.provide]`): package
/// name -> version provided.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProvideSection {
    /// Package name -> version. Flattened, so entries are the table's keys.
    /// Sorted, so the table does not reorder between writes.
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub entries: BTreeMap<String, String>,
}

impl ProvideSection {
    /// Returns `true` when the package provides nothing.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl From<BTreeMap<String, String>> for ProvideSection {
    fn from(entries: BTreeMap<String, String>) -> Self {
        Self { entries }
    }
}

impl FromIterator<(String, String)> for ProvideSection {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(iter: I) -> Self {
        Self {
            entries: iter.into_iter().collect(),
        }
    }
}

/// Arbitrary extra data from a package's `composer.json` (`[packages.extra]`),
/// carried verbatim as the raw JSON value.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExtraSection {
    /// The raw extra value (typically a JSON object).
    pub data: serde_json::Value,
}

/// A single package entry in `phast.lock`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PackageDef {
    /// Canonical `vendor/package` name, lowercased.
    pub name: String,

    /// Resolved version string, e.g. `1.2.3`.
    pub version: String,

    /// Directory of a workspace member, relative to the workspace root
    /// (forward slashes). Present only on member entries; its presence is
    /// what marks an entry as a workspace member. Member entries carry no
    /// `dist` or `source`, since there is nothing to fetch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// Runtime dependencies declared by the package.
    #[serde(default, skip_serializing_if = "RequireSection::is_empty")]
    pub require: RequireSection,

    /// Dev dependencies, recorded for workspace member entries only so the
    /// member's dev edges stay separable for `--no-dev`. External packages
    /// never carry this field.
    #[serde(
        rename = "require-dev",
        default,
        skip_serializing_if = "RequireSection::is_empty"
    )]
    pub require_dev: RequireSection,

    /// Distribution archive (zip/tar) to download for this package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dist: Option<DistSection>,

    /// VCS source reference, present when the package is installed from source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,

    /// Composer package type, e.g. `library`, `project`, `composer-plugin`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub package_type: Option<String>,

    /// Short human-readable description from the package's composer.json.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Package homepage URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,

    /// Release timestamp in ISO 8601 format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,

    /// Packagist notification URL called when the package is downloaded.
    #[serde(rename = "notification-url", skip_serializing_if = "Option::is_none")]
    pub notification_url: Option<String>,

    /// SPDX license identifiers declared by the package.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub license: Vec<String>,

    /// Searchable keywords declared by the package.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,

    /// Author entries from the package's composer.json.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<Author>,

    /// Support links (issues, source, docs, etc.) declared by the package.
    #[serde(default, skip_serializing_if = "Support::is_empty")]
    pub support: Support,

    /// Arbitrary extra data from the package's composer.json.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<ExtraSection>,

    /// AutoloadSection rules used to generate `vendor/autoload.php`.
    #[serde(default)]
    pub autoload: AutoloadSection,

    /// Virtual packages this package provides (e.g. `acme/log-implementation`).
    #[serde(default, skip_serializing_if = "ProvideSection::is_empty")]
    pub provide: ProvideSection,

    /// Executable scripts shipped by this package, relative to the package root.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bin: Vec<String>,
}

impl PackageDef {
    /// Whether this entry is a workspace member rather than an installable package.
    ///
    /// Member entries describe local source trees: they are never downloaded
    /// and must not be looked up on a registry.
    pub fn is_member(&self) -> bool {
        self.path.is_some()
    }

    /// Whether this package comes from a local `path` repository.
    ///
    /// Path packages are materialized into `vendor/` by symlink rather than
    /// downloaded and extracted like an archive.
    pub fn is_path(&self) -> bool {
        matches!(&self.dist, Some(dist) if dist.dist_type == DistType::Path)
    }
}

/// Archive format for a distribution download.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DistType {
    /// Zip archive, which is what Packagist serves.
    Zip,
    /// A local path repository: the package lives in a directory on disk and is
    /// materialized by symlink (or copy), not downloaded. `DistSection::url` holds
    /// the source directory relative to the project root.
    Path,
    /// An unrecognized format, preserved as-is for forward compatibility.
    #[serde(untagged)]
    Other(String),
}

/// Distribution archive descriptor for a locked package.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistSection {
    /// Archive format.
    #[serde(rename = "type")]
    pub dist_type: DistType,

    /// URL to download the archive from.
    pub url: String,

    /// VCS reference (commit hash or tag) the archive was built from.
    pub reference: Option<String>,

    /// SHA-1 checksum of the archive as provided by Packagist (their format, not ours).
    pub shasum: Option<String>,
}

impl DistSection {
    /// The download URL with Composer placeholders resolved.
    ///
    /// Packagist emits these for packages hosted off GitHub
    /// e.g. `.../archive/%prettyVersion%.zip`:
    ///     `%prettyVersion%` and `%version%` become `version`,
    ///     `%reference%` becomes the dist's reference.
    ///
    /// A URL with no placeholder is returned unchanged.
    pub fn download_url(&self, version: &str) -> String {
        let url = self
            .url
            .replace("%prettyVersion%", version)
            .replace("%version%", version);

        let Some(reference) = &self.reference else {
            return url;
        };

        url.replace("%reference%", reference)
    }
}

/// VCS source descriptor, used when installing a package directly from version control.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceRef {
    /// VCS backend type.
    #[serde(rename = "type")]
    pub vcs_type: VcsType,

    /// Repository URL.
    pub url: String,

    /// Commit hash, branch, or tag to check out.
    pub reference: String,
}

/// Autoload rules stored in `phast.lock` for a single package.
///
/// Cached here so that autoloader generation does not need to re-read every
/// package's `composer.json` at install time.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AutoloadSection {
    /// PSR-4 namespace prefix -> list of relative base directories.
    /// Sorted, so the table does not reorder between writes.
    #[serde(rename = "psr-4", default, skip_serializing_if = "BTreeMap::is_empty")]
    pub psr4: BTreeMap<String, Vec<String>>,

    /// PSR-0 namespace prefix -> list of relative base directories.
    /// Sorted, so the table does not reorder between writes.
    #[serde(rename = "psr-0", default, skip_serializing_if = "BTreeMap::is_empty")]
    pub psr0: BTreeMap<String, Vec<String>>,

    /// Paths to scan for class definitions (files or directories).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classmap: Vec<String>,

    /// Files to require unconditionally at autoloader bootstrap time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,

    /// Path patterns skipped while building the classmap.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
}

impl AutoloadSection {
    /// Returns `true` when no autoload rules have been set.
    pub fn is_empty(&self) -> bool {
        self.psr4.is_empty()
            && self.psr0.is_empty()
            && self.classmap.is_empty()
            && self.files.is_empty()
            && self.exclude.is_empty()
    }
}

impl From<Autoload> for AutoloadSection {
    fn from(autoload: Autoload) -> Self {
        Self {
            psr4: autoload.psr4,
            psr0: autoload.psr0,
            classmap: autoload.classmap,
            files: autoload.files,
            exclude: autoload.exclude,
        }
    }
}
