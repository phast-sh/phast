//! `composer.json` deserialization.
//!
//! Field names mirror Composer's schema, so this layer transcribes rather than interprets.
//!
//! Where Composer accepts a value in more than one shape, a PSR-4 path being either
//! `"App\\": "src/"` or `"App\\": ["src/", "lib/"]`, the field takes a `serde_json::Value`
//! and `into_manifest` settles the shape. No custom deserializers.
//!
//! `require` mixes platform packages in with the rest, `"php": ">=8.1"` beside `"psr/log": "^3.0"`.
//! Both land in one constraint map and are told apart by name later.

use super::{Autoload, BasicAuth, Config, Manifest, PlatformCheck, Repository, Script};

use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

use indexmap::IndexMap;
use serde::Deserialize;
use thiserror::Error;

/// Raw deserialization target for `composer.json`.
///
/// Field names follow the Composer schema exactly. After parsing, call
/// `into_manifest` to obtain the unified `Manifest` used by the rest of Phast.
#[rustfmt::skip]
#[derive(Debug, Deserialize)]
pub struct ComposerManifest {
    /// Canonical `vendor/package` name.
    pub name: Option<String>,

    /// Explicit version. Usually omitted in published packages (Composer derives
    /// it from VCS tags), but valid schema and needed for path-repository packages
    /// that must declare a concrete version.
    pub version: Option<String>,

    /// Short human-readable description of the project.
    pub description: Option<String>,

    /// Composer package type, e.g. `library`, `project`, `composer-plugin`.
    #[serde(rename = "type")]
    pub package_type: Option<String>,

    /// SPDX license identifier, e.g. `MIT` or `Apache-2.0`.
    pub license: Option<String>,

    /// Runtime dependencies: package name -> version constraint string.
    /// `IndexMap` preserves the author's ordering so `migrate` is faithful.
    #[serde(default)]
    pub require: IndexMap<String, String>,

    /// Dev-only dependencies: package name -> version constraint string.
    #[serde(rename = "require-dev", default)]
    pub require_dev: IndexMap<String, String>,

    /// `conflict`: incompatible packages (maps directly to PubGrub incompatibilities).
    #[serde(default)]
    pub conflict: IndexMap<String, String>,

    /// `replace`: this package replaces another (maps to resolver incompatibilities).
    #[serde(default)]
    pub replace: IndexMap<String, String>,

    /// `provide`: this package provides a virtual interface.
    #[serde(default)]
    pub provide: IndexMap<String, String>,

    /// `suggest`: informational only, not resolved.
    #[serde(default)]
    pub suggest: IndexMap<String, String>,

    /// Resolver and platform configuration.
    #[serde(default)]
    pub config: ComposerConfig,

    /// Autoload rules for production code.
    #[serde(default)]
    pub autoload: ComposerAutoload,

    /// Autoload rules for dev code (tests, tooling).
    #[serde(rename = "autoload-dev", default)]
    pub autoload_dev: ComposerAutoload,

    /// Lowest stability accepted during resolution, e.g. `"stable"` or `"dev"`.
    /// Root-level field in the Composer schema (not inside `config`).
    #[serde(rename = "minimum-stability", default)]
    pub minimum_stability: Option<String>,

    /// When `true`, prefer stable releases over pre-releases of equal constraint fit.
    /// Root-level field in the Composer schema (not inside `config`).
    #[serde(rename = "prefer-stable", default)]
    pub prefer_stable: bool,

    /// Lifecycle scripts keyed by event name, e.g. `post-install-cmd`.
    #[serde(default)]
    pub scripts: IndexMap<String, Script>,

    /// Additional package repositories declared in the manifest.
    #[serde(default)]
    pub repositories: Vec<serde_json::Value>,

    /// Arbitrary extra data passed through from the manifest unchanged.
    #[serde(default)]
    pub extra: serde_json::Value,

    // Informational fields: parsed for completeness, not used by the resolver or installer.
    // ---------------------------------------------------------------------------------------------
    /// Searchable keywords describing the package.
    #[serde(default)]
    pub keywords: Vec<String>,

    /// Package homepage URL.
    pub homepage: Option<String>,

    /// Release timestamp in ISO 8601 format.
    pub time: Option<String>,

    /// Author entries from the manifest.
    pub authors: Option<Vec<serde_json::Value>>,

    /// Support links: issues, source, docs, etc.
    pub support: Option<serde_json::Value>,

    /// Funding links for the project.
    pub funding: Option<Vec<serde_json::Value>>,
    // ---------------------------------------------------------------------------------------------
}

/// Autoload section with polymorphic path values.
///
/// PSR-4 and PSR-0 map a namespace prefix to one or more paths.
/// `serde_json::Value` is used because the value can be a string or an array;
/// normalization to `Vec<String>` happens in `into_manifest`.
#[derive(Debug, Default, Deserialize)]
pub struct ComposerAutoload {
    /// PSR-4 namespace prefix -> path(s), raw from JSON (string or array).
    #[serde(rename = "psr-4", default)]
    pub psr4: IndexMap<String, serde_json::Value>,

    /// PSR-0 namespace prefix -> path(s), raw from JSON (string or array).
    #[serde(rename = "psr-0", default)]
    pub psr0: IndexMap<String, serde_json::Value>,

    /// Paths to scan for class definitions.
    #[serde(default)]
    pub classmap: Vec<String>,

    /// Files to require unconditionally at autoloader bootstrap.
    #[serde(default)]
    pub files: Vec<String>,

    /// Paths excluded from classmap scanning.
    #[serde(rename = "exclude-from-classmap", default)]
    pub exclude_from_classmap: Vec<String>,
}

/// HTTP basic auth entry inside `composer.json` `"config"`.
#[derive(Debug, Clone, Deserialize)]
pub struct ComposerBasicAuth {
    /// Login username.
    pub username: String,
    /// Login password or personal access token.
    pub password: String,
}

/// Raw deserialization target for the `"config"` section of `composer.json`.
#[derive(Debug, Default, Deserialize)]
pub struct ComposerConfig {
    /// Platform overrides: `{"php": "8.2.0", "ext-json": "1.0"}`.
    #[serde(default)]
    pub platform: IndexMap<String, String>,

    /// Custom vendor directory path. Deprecated but still common in the wild.
    #[serde(rename = "vendor-dir", default)]
    pub vendor_dir: Option<String>,

    /// Custom binary directory path.
    #[serde(rename = "bin-dir", default)]
    pub bin_dir: Option<String>,

    /// When `true`, keep `require` entries sorted alphabetically.
    #[serde(rename = "sort-packages", default)]
    pub sort_packages: bool,

    /// Inline HTTP basic auth credentials keyed by hostname.
    #[serde(rename = "http-basic", default)]
    pub http_basic: HashMap<String, ComposerBasicAuth>,

    /// Inline bearer tokens keyed by hostname.
    #[serde(default)]
    pub bearer: HashMap<String, String>,

    /// Transport security toggle. `None` means use the secure default (reject
    /// plaintext `http://`); `Some(false)` opts out for trusted internal registries.
    #[serde(rename = "secure-http", default)]
    pub secure_http: Option<bool>,
}

/// Errors that can occur when parsing or converting a `composer.json`.
#[derive(Debug, Error)]
pub enum ComposerManifestError {
    #[error("failed to parse composer.json: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("invalid constraint for '{package}': {reason}")]
    InvalidConstraint { package: String, reason: String },

    #[error("invalid \"config\" value for '{key}': {reason}")]
    InvalidConfig { key: String, reason: String },
}

impl FromStr for ComposerManifest {
    type Err = ComposerManifestError;

    /// Parse a `composer.json` from its raw JSON string.
    fn from_str(string: &str) -> Result<Self, Self::Err> {
        Ok(serde_json::from_str(string)?)
    }
}

#[rustfmt::skip]
impl ComposerManifest {
    /// Convert into the unified `Manifest` type.
    pub fn into_manifest(self) -> Result<Manifest, ComposerManifestError> {
        use super::convert::{
            normalize_json_paths,
            parse_json_repository,
            parse_require_map,
            parse_version_spec,
            parse_stability,
        };

        let require = parse_require_map(self.require).map_err(|(pkg, reason)| {
            ComposerManifestError::InvalidConstraint { package: pkg, reason }
        })?;

        let require_dev = parse_require_map(self.require_dev).map_err(|(pkg, reason)| {
            ComposerManifestError::InvalidConstraint { package: pkg, reason }
        })?;

        let conflict = parse_require_map(self.conflict).map_err(|(pkg, reason)| {
            ComposerManifestError::InvalidConstraint { package: pkg, reason }
        })?;

        // replace/provide are version specs, not constraints: kept verbatim
        // (self.version, *, exact), unparseable ranges skipped.
        let replace = parse_version_spec(self.replace);
        let provide = parse_version_spec(self.provide);

        let minimum_stability = parse_stability(self.minimum_stability.as_deref())
            .map_err(|reason| ComposerManifestError::InvalidConfig {
                key: "minimum-stability".into(),
                reason,
            })?;

        let mut autoload_psr4 = BTreeMap::new();
        for (prefix, value) in self.autoload.psr4 {
            let paths = normalize_json_paths(&value).map_err(|reason| {
                ComposerManifestError::InvalidConstraint { package: prefix.clone(), reason }
            })?;
            autoload_psr4.insert(prefix, paths);
        }

        let mut autoload_psr0 = BTreeMap::new();
        for (prefix, value) in self.autoload.psr0 {
            let paths = normalize_json_paths(&value).map_err(|reason| {
                ComposerManifestError::InvalidConstraint { package: prefix.clone(), reason }
            })?;
            autoload_psr0.insert(prefix, paths);
        }

        let mut autoload_dev_psr4 = BTreeMap::new();
        for (prefix, value) in self.autoload_dev.psr4 {
            let paths = normalize_json_paths(&value).map_err(|reason| {
                ComposerManifestError::InvalidConstraint { package: prefix.clone(), reason }
            })?;
            autoload_dev_psr4.insert(prefix, paths);
        }

        let mut autoload_dev_psr0 = BTreeMap::new();
        for (prefix, value) in self.autoload_dev.psr0 {
            let paths = normalize_json_paths(&value).map_err(|reason| {
                ComposerManifestError::InvalidConstraint { package: prefix.clone(), reason }
            })?;
            autoload_dev_psr0.insert(prefix, paths);
        }

        let mut repositories: Vec<Repository> = Vec::new();
        for repository in self.repositories {
            if let Some(repo) = parse_json_repository(repository).map_err(|reason| {
                ComposerManifestError::InvalidConstraint { package: "repository".into(), reason }
            })? {
                repositories.push(repo);
            }
        }

        let http_basic = self
            .config
            .http_basic
            .into_iter()
            .map(|(host, config)| {
                (host, BasicAuth {
                    username: config.username,
                    password: config.password,
                })
            })
            .collect();

        let secure_http = self
            .config
            .secure_http
            .unwrap_or(true);

        Ok(Manifest {
            name: self.name,
            version: self.version,
            description: self.description,
            package_type: self.package_type,
            license: self.license,
            require,
            require_dev,
            conflict,
            replace,
            provide,
            config: Config {
                minimum_stability,
                minimum_package_age: None,
                prefer_stable: self.prefer_stable,
                classmap_authoritative: false,
                platform: self.config.platform.into_iter().collect(),
                platform_check: PlatformCheck::default(),
                http_basic,
                bearer: self.config.bearer,
                secure_http,
            },
            autoload: Autoload {
                psr4: autoload_psr4,
                psr0: autoload_psr0,
                classmap: self.autoload.classmap,
                files: self.autoload.files,
                exclude: self.autoload.exclude_from_classmap,
            },
            autoload_dev: Autoload {
                psr4: autoload_dev_psr4,
                psr0: autoload_dev_psr0,
                classmap: self.autoload_dev.classmap,
                files: self.autoload_dev.files,
                exclude: self.autoload_dev.exclude_from_classmap,
            },
            scripts: self.scripts.into_iter().collect(),
            repositories,
            sources: HashMap::new(),
            extra: self.extra,
        })
    }
}
