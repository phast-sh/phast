//! `phast.toml` serialization and deserialization.
//!
//! `PhastManifest` serves as both the read model (deserializing from TOML via
//! `FromStr`) and the write model (serializing to TOML via `toml::to_string_pretty`).
//! One struct for both directions, so anything Phast writes is by construction something
//! Phast can read back.
//!
//! ## Format
//!
//! Every section is optional and omitted when empty. The example below shows the
//! full set of sections a `phast.toml` may contain.
//!
//! ```toml
//! [package]
//! name        = "acme/app"
//! version     = "1.0.0"
//! description = "Example application"
//! type        = "project"
//! license     = "MIT"
//! homepage    = "https://acme.example"
//! keywords    = ["example"]
//!
//! # Informational metadata (authors, support, funding): preserved, never resolved.
//! [[package.authors]]
//! name  = "Acme Engineering"
//! email = "dev@acme.example"
//!
//! [package.support]
//! issues = "https://github.com/acme/app/issues"
//! source = "https://github.com/acme/app"
//!
//! [[package.funding]]
//! type = "github"
//! url  = "https://github.com/sponsors/acme"
//!
//! [require]
//! php              = ">=8.2"
//! "acme/framework" = "^11.0"
//!
//! [require-dev]
//! "acme/test-tools" = "^11.0"
//!
//! [provide]
//! "acme/log-implementation" = "3.0"
//!
//! [replace]
//! "acme/legacy" = "self.version"
//!
//! [conflict]
//! "acme/old-sdk" = "<2.0"
//!
//! [suggest]
//! "ext-redis" = "For faster caching"
//!
//! # Autoload: array values (classmap, files) precede the psr sub-tables.
//! [autoload]
//! files   = ["src/helpers.php"]
//! exclude = ["src/Legacy/"]
//!
//! [autoload.psr-4]
//! "Acme\\App\\" = "src/"
//!
//! [autoload-dev.psr-4]
//! "Acme\\App\\Tests\\" = "tests/"
//!
//! [config]
//! minimum-stability      = "stable"
//! prefer-stable          = true
//! sort-packages          = true
//! classmap-authoritative = false
//! platform-check         = "php"
//!
//! [config.platform]
//! php = "8.2"
//!
//! # Multi-package endpoints; `[sources]` pins a package to one by name.
//! [[repositories]]
//! name = "internal"
//! type = "composer"
//! url  = "https://packages.acme.example"
//!
//! [sources]
//! "acme/legacy" = { repository = "internal" }
//!
//! [scripts]
//! post-install-cmd = ["@php scripts/post-install.php"]
//!
//! [workspace]
//! members = ["libs/*"]
//!
//! [extra]
//! branch-alias = { "dev-main" = "1.x-dev" }
//! ```

use super::convert::{parse_duration, parse_platform_check, parse_stability};
use super::formatter::{self, FormatOptions};
use super::{Autoload, Config, GitRef, Repository, RepositoryType, Source};
use super::{Manifest, Script};

use crate::package::PackageName;
use crate::shared::{Author, Support};

use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

use anyhow::Context;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A `phast.toml` manifest: what a project is, what it depends on,
/// and how Phast should resolve and install it.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct PhastManifest {
    /// Project identity and metadata.
    #[serde(default, skip_serializing_if = "PackageSection::is_empty")]
    pub package: PackageSection,

    /// Runtime dependencies.
    #[serde(default, skip_serializing_if = "RequireSection::is_empty")]
    pub require: RequireSection,

    /// Dev-only dependencies, for development and testing.
    #[serde(
        rename = "require-dev",
        default,
        skip_serializing_if = "RequireSection::is_empty"
    )]
    pub require_dev: RequireSection,

    /// Virtual packages this project provides.
    #[serde(default, skip_serializing_if = "ProvideSection::is_empty")]
    pub provide: ProvideSection,

    /// Packages this project replaces.
    #[serde(default, skip_serializing_if = "ReplaceSection::is_empty")]
    pub replace: ReplaceSection,

    /// Package versions this project is incompatible with.
    #[serde(default, skip_serializing_if = "ConflictSection::is_empty")]
    pub conflict: ConflictSection,

    /// Optional package suggestions.
    #[serde(default, skip_serializing_if = "SuggestSection::is_empty")]
    pub suggest: SuggestSection,

    /// Autoload rules for the project's own code.
    #[serde(default, skip_serializing_if = "AutoloadSection::is_empty")]
    pub autoload: AutoloadSection,

    /// Autoload rules applied only when dev dependencies are installed.
    #[serde(
        rename = "autoload-dev",
        default,
        skip_serializing_if = "AutoloadSection::is_empty"
    )]
    pub autoload_dev: AutoloadSection,

    /// Lifecycle hooks and named tasks.
    #[serde(default, skip_serializing_if = "ScriptsSection::is_empty")]
    pub scripts: ScriptsSection,

    /// Resolver and install settings.
    #[serde(default, skip_serializing_if = "ConfigSection::is_empty")]
    pub config: ConfigSection,

    /// Extra package registries and single-package endpoints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<RepositoryDef>,

    /// Per-package source overrides.
    #[serde(default, skip_serializing_if = "SourcesSection::is_empty")]
    pub sources: SourcesSection,

    /// Workspace root configuration and its members.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceSection>,

    /// Free-form data for tooling; not interpreted by Phast.
    #[serde(default, skip_serializing_if = "ExtraSection::is_empty")]
    pub extra: ExtraSection,
}

/// Project identity and metadata.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct PackageSection {
    /// The project's `vendor/package` name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The project's version, e.g. `"1.0.0"`. Required for workspace members that other
    /// members depend on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// A short description of the project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Package type, e.g. `library`, `project`, `composer-plugin`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub package_type: Option<String>,

    /// SPDX license identifier, e.g. `MIT` or `Apache-2.0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    /// The project's homepage URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,

    /// Searchable tags used when publishing to a registry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,

    /// The project's authors.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<Author>,

    /// Support channels, keyed by channel name (issues, source, docs, ...).
    #[serde(default, skip_serializing_if = "Support::is_empty")]
    pub support: Support,

    /// Funding or sponsorship links.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub funding: Vec<Funding>,
}

impl PackageSection {
    /// Returns `true` when no field has been set, so the section is omitted
    /// from the serialized output entirely.
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.version.is_none()
            && self.description.is_none()
            && self.package_type.is_none()
            && self.license.is_none()
            && self.homepage.is_none()
            && self.keywords.is_empty()
            && self.authors.is_empty()
            && self.support.is_empty()
            && self.funding.is_empty()
    }
}

/// A funding or sponsorship link.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Funding {
    /// Funding platform, e.g. `"github"`, `"patreon"`, or `"open_collective"`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub funding_type: Option<String>,

    /// URL of the funding or sponsorship page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A dependency value: a version constraint (e.g. `"^7.0"`), or `{ workspace = true }`
/// to inherit the constraint from the workspace root.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum DependencySpec {
    /// Plain version constraint, e.g. `"^7.0"`.
    Version(String),
    /// Inherit the version constraint from `[workspace.dependencies]`.
    Workspace(WorkspaceRef),
}

/// Inherit a dependency's version constraint from the workspace root.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceRef {
    /// Set to `true` to take the constraint from the root's `[workspace.dependencies]`.
    pub workspace: bool,
}

/// Project dependencies: each package name mapped to a version constraint.
///
/// Keys are `vendor/package` names or platform packages like `php` and `ext-json`.
/// A value is a version constraint (e.g. `"^7.0"`), or `{ workspace = true }`
/// to inherit the constraint from the workspace root.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct RequireSection {
    /// Each package name mapped to its version constraint.
    #[serde(flatten, default, skip_serializing_if = "IndexMap::is_empty")]
    pub dependencies: IndexMap<String, DependencySpec>,
}

impl RequireSection {
    /// Returns `true` when no dependencies have been declared.
    pub fn is_empty(&self) -> bool {
        self.dependencies.is_empty()
    }
}

/// Virtual packages this project provides, each package name mapped to the version
/// provided (`*` or `self.version` allowed).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct ProvideSection {
    /// Package name -> version this project provides it at.
    #[serde(flatten, default, skip_serializing_if = "IndexMap::is_empty")]
    pub entries: IndexMap<String, String>,
}

impl ProvideSection {
    /// Returns `true` when nothing is provided.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Packages this project supplies itself, each package name mapped to the version
/// replaced (`*` or `self.version` allowed).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct ReplaceSection {
    /// Package name -> version spec this project replaces it at.
    #[serde(flatten, default, skip_serializing_if = "IndexMap::is_empty")]
    pub entries: IndexMap<String, String>,
}

impl ReplaceSection {
    /// Returns `true` when nothing is replaced.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Packages this project is incompatible with, each package name
/// mapped to a constraint of versions to exclude.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct ConflictSection {
    /// Package name -> incompatible version constraint.
    #[serde(flatten, default, skip_serializing_if = "IndexMap::is_empty")]
    pub entries: IndexMap<String, String>,
}

impl ConflictSection {
    /// Returns `true` when no conflicts are declared.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Optional packages, each package name mapped to a human-readable reason.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct SuggestSection {
    /// Package name -> reason it is suggested.
    #[serde(flatten, default, skip_serializing_if = "IndexMap::is_empty")]
    pub entries: IndexMap<String, String>,
}

impl SuggestSection {
    /// Returns `true` when nothing is suggested.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Autoloading rules for the project's own code, compiled into the generated autoloader
/// alongside every dependency's rules. A PSR prefix maps to one path or a list of paths.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct AutoloadSection {
    /// A PSR-4 namespace prefix mapped to its path (or list of paths).
    /// Insertion-ordered, so a handwritten order survives a round trip.
    #[serde(rename = "psr-4", default, skip_serializing_if = "IndexMap::is_empty")]
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "crate::schema::psr_map_schema")
    )]
    pub psr4: IndexMap<String, toml::Value>,

    /// A PSR-0 namespace prefix mapped to its path (or list of paths).
    /// Insertion-ordered, so a handwritten order survives a round trip.
    #[serde(rename = "psr-0", default, skip_serializing_if = "IndexMap::is_empty")]
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "crate::schema::psr_map_schema")
    )]
    pub psr0: IndexMap<String, toml::Value>,

    /// Directories or files scanned for class definitions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classmap: Vec<String>,

    /// Files always loaded by the autoloader, e.g. global helper functions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,

    /// Path patterns kept out of the generated classmap.
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

/// Resolver and install settings.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct ConfigSection {
    /// Lowest release stability the resolver may select. Defaults to `stable`.
    #[serde(
        rename = "minimum-stability",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "crate::schema::stability_schema")
    )]
    pub minimum_stability: Option<String>,

    /// Minimum age a package version must have before it can be selected.
    ///
    /// Expressed as a duration string: `"24h"`, `"7d"`, `"2w"`.
    /// Versions published more recently than this threshold are excluded.
    #[serde(
        rename = "minimum-package-age",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(
        feature = "schema",
        schemars(schema_with = "crate::schema::duration_schema")
    )]
    pub minimum_package_age: Option<String>,

    /// When `true`, prefer stable releases over pre-releases of equal constraint fit.
    #[serde(rename = "prefer-stable", default, skip_serializing_if = "is_false")]
    pub prefer_stable: bool,

    /// Generate the autoloader in classmap-authoritative mode: trust
    /// the classmap exclusively and drop the PSR filesystem fallback.
    #[serde(
        rename = "classmap-authoritative",
        default,
        skip_serializing_if = "is_false"
    )]
    pub classmap_authoritative: bool,

    /// When `true`, sort every dependency section on each write, platform packages
    /// first and then alphabetically.
    #[serde(rename = "sort-packages", default, skip_serializing_if = "is_false")]
    pub sort_packages: bool,

    /// Platform version overrides used during resolution.
    #[serde(default, skip_serializing_if = "Platform::is_empty")]
    pub platform: Platform,

    /// Reject plaintext `http://` registry URLs. Enabled by default;
    /// set to `false` to allow a trusted internal registry served over HTTP.
    #[serde(
        rename = "secure-http",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub secure_http: Option<bool>,

    /// Whether `platform_check.php` is generated, and how much it verifies:
    /// `"php"`, `"all"`, or `"off"`. Absent means the `php` default.
    #[serde(
        rename = "platform-check",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub platform_check: Option<String>,

    /// Plugin execution allowlist: `"vendor/plugin" = true` to allow, `false` to block.
    /// Reserved: an entry here is recorded and has no effect on what runs.
    #[serde(
        rename = "allow-plugins",
        default,
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub allow_plugins: IndexMap<String, bool>,
}

impl ConfigSection {
    /// Returns `true` when all config fields are at their defaults.
    pub fn is_empty(&self) -> bool {
        self.allow_plugins.is_empty()
            && self.minimum_stability.is_none()
            && self.minimum_package_age.is_none()
            && !self.prefer_stable
            && !self.classmap_authoritative
            && !self.sort_packages
            && self.platform.is_empty()
            && self.secure_http.is_none()
    }
}

/// Platform version overrides, so resolution behaves the same
/// regardless of the locally installed PHP and extensions.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Platform {
    /// Override the PHP version used during resolution, e.g. `"8.2"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub php: Option<String>,

    /// Extension overrides, e.g. `ext-intl = "1.0"`.
    #[serde(flatten, default, skip_serializing_if = "IndexMap::is_empty")]
    pub extensions: IndexMap<String, String>,
}

impl Platform {
    /// Returns `true` when no platform overrides have been set.
    pub fn is_empty(&self) -> bool {
        self.php.is_none() && self.extensions.is_empty()
    }
}

/// Lifecycle hooks and named tasks. A key is a lifecycle event (e.g. `post-install`) or
/// a task name you run with `phast run <name>`; its value is a single command or a list
/// of commands run in order.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct ScriptsSection {
    /// All scripts, keyed by event or task name.
    /// Insertion-ordered, so a handwritten order survives a round trip.
    #[serde(flatten, default, skip_serializing_if = "IndexMap::is_empty")]
    pub entries: IndexMap<String, Script>,
}

impl ScriptsSection {
    /// Returns `true` when no scripts have been defined.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// An extra registry or single-package source endpoint.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryDef {
    /// What kind of repository this is.
    #[serde(rename = "type")]
    pub repo_type: RepositoryType,

    /// Repository URL or local path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Name used to reference this repository from `[sources]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// When `true`, packages not listed in `[sources]` are fetched from this repository.
    #[serde(default, skip_serializing_if = "is_false")]
    pub default: bool,
}

/// Where individual packages come from, mapping each `"vendor/package"` name to its
/// source: a named `[[repositories]]` endpoint, a single-package git source, a local
/// path, or an inline package.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct SourcesSection {
    /// Per-package source specs, keyed by `"vendor/package"`.
    #[serde(flatten, default, skip_serializing_if = "IndexMap::is_empty")]
    pub entries: IndexMap<String, SourceSpec>,
}

impl SourcesSection {
    /// Returns `true` when no source specs have been defined.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A single `[sources]` entry: where one package comes from. Which fields you set
/// selects the kind of source:
///
/// - `repository` - fetch from the named `[[repositories]]` endpoint.
/// - `path` - a local package on disk (optionally with `symlink` and `version`).
/// - `url` with `version` - a self-describing inline package.
/// - `url` with no `version` - a git source, whose metadata is read from the repository.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SourceSpec {
    /// Name of the `[[repositories]]` endpoint this package is fetched from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,

    /// Clone URL of a single-package git source or inline package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Branch to check out (git source).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,

    /// Tag to check out (git source).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,

    /// Exact commit reference to check out (git source or inline package).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,

    /// Filesystem path to a local package directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// Symlink a path package into `vendor/`. Only `true` is accepted: copying is
    /// not supported, so `symlink = false` is rejected rather than silently ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symlink: Option<bool>,

    /// The package's version. Supplies the version for a path source,
    /// or (with `url`) marks a self-describing inline package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl SourceSpec {
    /// Classify this flat surface record into a domain [`Source`], given the
    /// package `name` it is keyed by. Errors when the fields name no known shape.
    #[rustfmt::skip]
    fn into_source(self, name: &str) -> Result<Source, String> {
        // A named endpoint: `{ repository = "..." }`.
        if let Some(repository) = self.repository {
            return Ok(Source::Repository {
                repository,
            });
        }

        // A local path package: `{ path = "...", symlink?, version? }`.
        if let Some(path) = self.path {
            // TODO: copying is not implemented, so refuse `symlink = false`
            //       rather than symlink regardless.
            if self.symlink == Some(false) {
                return Err(format!(
                    "source for '{name}' sets `symlink = false`, which is not supported: \
                     a path package is always materialized by symlink",
                ));
            }

            return Ok(Source::Path {
                path,
                version: self.version,
            });
        }

        // Anything left must carry a git URL.
        let Some(url) = self.url else {
            return Err(format!(
                "source for '{name}' must set one of `repository`, `url`, or `path`",
            ));
        };

        // A declared `version` makes it a self-describing inline package (the spec
        // *is* the package); without one it is a git source read from the repo.
        if let Some(version) = self.version {
            let package = serde_json::json!({
                "name": name,
                "version": version,
                "source": {
                    "type": "git",
                    "url": url,
                    "reference": self.reference,
                },
            });
            return Ok(Source::Package { package });
        }

        let reference = match (
            self.branch,
            self.tag,
            self.reference,
        ) {
            (Some(branch), _, _) => GitRef::Branch(branch),
            (_, Some(tag), _)    => GitRef::Tag(tag),
            (_, _, Some(commit)) => GitRef::Reference(commit),
            (None, None, None)   => GitRef::Default,
        };

        Ok(Source::Git { url, reference })
    }
}

/// Marks this manifest as a workspace root and configures its members.
/// Present only in a root manifest.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceSection {
    /// Glob patterns that identify member directories, relative to the workspace root.
    /// Each matched directory must contain a `phast.toml` to be treated as a member.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,

    /// Glob patterns that exclude directories from membership even if they match `members`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,

    /// Shared version constraints that members inherit with `{ workspace = true }`. Values
    /// are plain version constraints; whether a member treats one as dev-only is decided in
    /// its own `[require]` or `[require-dev]`.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub dependencies: IndexMap<String, String>,
}

/// Free-form data for plugins and framework tooling.
/// Phast preserves it but does not interpret it.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct ExtraSection {
    /// All extra data, keyed by arbitrary names.
    /// Insertion-ordered, so a handwritten order survives a round trip.
    #[serde(flatten, default, skip_serializing_if = "IndexMap::is_empty")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "IndexMap<String, serde_json::Value>")
    )]
    pub data: IndexMap<String, toml::Value>,
}

impl ExtraSection {
    /// Returns `true` when no extra data has been set.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// Errors that can occur when parsing or converting a `phast.toml`.
#[derive(Debug, Error)]
pub enum PhastManifestError {
    #[error("failed to parse phast.toml: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("invalid constraint for '{package}': {reason}")]
    InvalidConstraint { package: String, reason: String },

    #[error("invalid [sources] entry: {reason}")]
    InvalidSource { reason: String },

    #[error("invalid [config] value for '{key}': {reason}")]
    InvalidConfig { key: String, reason: String },

    #[error("failed to convert [extra] section to JSON: {reason}")]
    InvalidExtra { reason: String },

    #[error("'{package}' uses {{ workspace = true }}, which only its workspace can resolve")]
    UnexpandedWorkspaceRef { package: String },
}

impl FromStr for PhastManifest {
    type Err = PhastManifestError;

    /// Parse a `phast.toml` from its raw TOML string.
    fn from_str(string: &str) -> Result<Self, Self::Err> {
        Ok(toml::from_str(string)?)
    }
}

#[rustfmt::skip]
impl PhastManifest {
    /// Serialize this manifest to formatted TOML, applying its own
    /// formatting config (e.g. `sort-packages`).
    ///
    /// Prepends a `#:schema` directive so editors bind the freshly written file to the
    /// published schema.
    pub fn to_toml(&self) -> anyhow::Result<String> {
        let toml = toml::to_string_pretty(self)
            .context("failed to serialize manifest")?;

        let schema = crate::meta::schema_url();
        let manifest = formatter::format(&toml, &FormatOptions {
            sort_packages: self.config.sort_packages,
        })?;

        Ok(format!("#:schema {schema}\n{manifest}"))
    }

    /// Format an existing `phast.toml` document, preserving comments and
    /// key order. Formatting config (e.g. `sort-packages`) is read from
    /// the document itself.
    pub fn format(toml: &str) -> anyhow::Result<String> {
        let manifest = Self::from_str(toml)
            .context("failed to parse phast.toml")?;

        formatter::format(toml, &FormatOptions {
            sort_packages: manifest.config.sort_packages,
        })
    }

    /// Convert into the unified `Manifest` type used by all Phast crates.
    ///
    /// Parses constraint strings, normalizes autoload path values, and converts
    /// platform overrides into the flat map expected by `Config`.
    pub fn into_manifest(self) -> Result<Manifest, PhastManifestError> {
        use super::convert::{
            normalize_toml_paths,
            parse_toml_repository,
            parse_require_map,
            parse_version_spec,
        };

        let deps = extract_versions(self.require.dependencies);
        let require = parse_require_map(deps?).map_err(|(pkg, reason)| {
            PhastManifestError::InvalidConstraint { package: pkg, reason }
        })?;

        let deps = extract_versions(self.require_dev.dependencies);
        let require_dev = parse_require_map(deps?).map_err(|(pkg, reason)| {
            PhastManifestError::InvalidConstraint { package: pkg, reason }
        })?;

        let conflict = parse_require_map(self.conflict.entries).map_err(|(pkg, reason)| {
            PhastManifestError::InvalidConstraint { package: pkg, reason }
        })?;

        // replace/provide are version specs, not constraints: self.version, *, or
        // an exact version, kept verbatim; unparseable ranges skipped.
        let replace = parse_version_spec(self.replace.entries);
        let provide = parse_version_spec(self.provide.entries);

        let minimum_stability = parse_stability(self.config.minimum_stability.as_deref())
            .map_err(|reason| PhastManifestError::InvalidConfig {
                key: "minimum-stability".into(),
                reason,
            })?;

        let minimum_package_age = parse_duration(self.config.minimum_package_age.as_deref())
            .map_err(|reason| PhastManifestError::InvalidConfig {
                key: "minimum-package-age".into(),
                reason,
            })?;

        let mut autoload_psr4 = BTreeMap::new();
        for (prefix, value) in self.autoload.psr4 {
            let paths = normalize_toml_paths(&value).map_err(|reason| {
                PhastManifestError::InvalidConstraint { package: prefix.clone(), reason }
            })?;
            autoload_psr4.insert(prefix, paths);
        }

        let mut autoload_psr0 = BTreeMap::new();
        for (prefix, value) in self.autoload.psr0 {
            let paths = normalize_toml_paths(&value).map_err(|reason| {
                PhastManifestError::InvalidConstraint { package: prefix.clone(), reason }
            })?;
            autoload_psr0.insert(prefix, paths);
        }

        let mut autoload_dev_psr4 = BTreeMap::new();
        for (prefix, value) in self.autoload_dev.psr4 {
            let paths = normalize_toml_paths(&value).map_err(|reason| {
                PhastManifestError::InvalidConstraint { package: prefix.clone(), reason }
            })?;
            autoload_dev_psr4.insert(prefix, paths);
        }

        let mut autoload_dev_psr0 = BTreeMap::new();
        for (prefix, value) in self.autoload_dev.psr0 {
            let paths = normalize_toml_paths(&value).map_err(|reason| {
                PhastManifestError::InvalidConstraint { package: prefix.clone(), reason }
            })?;
            autoload_dev_psr0.insert(prefix, paths);
        }

        let mut repositories: Vec<Repository> = Vec::new();
        for repository in self.repositories {
            let Some(repo) = parse_toml_repository(&repository).map_err(|reason| {
                PhastManifestError::InvalidConstraint { package: "repository".into(), reason }
            })? else {
                continue;
            };
            repositories.push(repo);
        }

        let mut sources: HashMap<PackageName, Source> = HashMap::new();
        for (name, spec) in self.sources.entries {
            let source = spec.into_source(&name).map_err(|reason| {
                PhastManifestError::InvalidSource { reason }
            })?;
            sources.insert(PackageName::new(&name), source);
        }

        // Build the flat platform map expected by Config.
        let mut platform = self.config.platform.extensions;
        if let Some(php) = self.config.platform.php {
            platform.insert("php".to_string(), php);
        }

        let platform_check = parse_platform_check(self.config.platform_check.as_deref())
            .map_err(|reason| PhastManifestError::InvalidConfig {
                key: "platform-check".into(),
                reason,
            })?;

        // Convert extra from toml::Value to serde_json::Value.
        // Fails if the TOML contains types with no JSON equivalent (e.g. datetimes).
        let extra = if self.extra.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::to_value(&self.extra.data).map_err(|reason| {
                PhastManifestError::InvalidExtra { reason: reason.to_string() }
            })?
        };

        let secure_http = self
            .config
            .secure_http
            .unwrap_or(true);

        Ok(Manifest {
            name: self.package.name,
            version: self.package.version,
            description: self.package.description,
            package_type: self.package.package_type,
            license: self.package.license,
            require,
            require_dev,
            conflict,
            replace,
            provide,
            config: Config {
                minimum_stability,
                minimum_package_age,
                prefer_stable: self.config.prefer_stable,
                classmap_authoritative: self.config.classmap_authoritative,
                platform: platform.into_iter().collect(),
                platform_check,
                http_basic: HashMap::new(),
                bearer: HashMap::new(),
                secure_http,
            },
            autoload: Autoload {
                psr0: autoload_psr0,
                psr4: autoload_psr4,
                classmap: self.autoload.classmap,
                files: self.autoload.files,
                exclude: self.autoload.exclude,
            },
            autoload_dev: Autoload {
                psr0: autoload_dev_psr0,
                psr4: autoload_dev_psr4,
                classmap: self.autoload_dev.classmap,
                files: self.autoload_dev.files,
                exclude: self.autoload_dev.exclude,
            },
            scripts: self.scripts.entries.into_iter().collect(),
            repositories,
            sources,
            extra,
        })
    }
}

/// Returns `true` when the value is `false`.
///
/// Used with `skip_serializing_if` to omit boolean fields that default to `false`,
/// keeping a written `phast.toml` free of entries that say nothing.
fn is_false(value: &bool) -> bool {
    !value
}

/// Extract plain version strings from a dependency map, erroring on any unresolved
/// workspace refs. Called by `into_manifest` before passing deps to `parse_require_map`.
#[rustfmt::skip]
fn extract_versions(
    deps: IndexMap<String, DependencySpec>,
) -> Result<HashMap<String, String>, PhastManifestError> {
    deps.into_iter()
        .map(|(name, spec)| match spec {
            DependencySpec::Version(v)   => Ok((name, v)),
            DependencySpec::Workspace(_) => Err(PhastManifestError::UnexpandedWorkspaceRef { package: name }),
        })
        .collect()
}
