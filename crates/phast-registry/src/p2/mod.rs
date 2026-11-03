//! Packagist v2 metadata format deserialization.
//!
//! The p2 API returns all versions of a package in a single JSON response.
//! When `"minified": "composer/2.0"` is present, each version entry inherits all
//! fields from the previous entry and carries only what changed, so the entries
//! are merged forward before the parsed list is returned.

pub mod convert;
pub mod parser;

use std::collections::HashMap;

use phast_core::lock::phast::DistType;
use phast_core::manifest::VcsType;

use serde::Deserialize;
use serde_json::Value::Object;
use serde_json::{Map, Value, from_value};

/// Top-level p2 response, from the registry's own `metadata-url` template,
/// `/p2/%package%.json` on packagist.org.
#[derive(Debug, Deserialize)]
pub struct P2Response {
    /// Map of package name to its raw version entries (one entry per version).
    pub packages: HashMap<String, Vec<Value>>,
    /// Present and set to `"composer/2.0"` when the minified format is used.
    pub minified: Option<String>,
}

/// A single fully-merged version entry from the p2 response.
///
/// In the minified format, entries are delivered as diffs. After calling
/// [`expand_versions`] each `P2Version` represents a complete snapshot for
/// one release.
#[derive(Debug, Deserialize, Clone)]
pub struct P2Version {
    /// Human-readable version string as published on Packagist (e.g. `"1.2.3"`).
    pub version: Option<String>,

    /// Normalized four-part version string (e.g. `"1.2.3.0"`). Preferred over
    /// `version` when parsing, as it is already in canonical form.
    #[serde(rename = "version-normalized")]
    pub version_normalized: Option<String>,

    /// Runtime dependencies: package name to constraint string.
    #[serde(default)]
    pub require: HashMap<String, String>,

    /// Development-only dependencies: package name to constraint string.
    #[serde(default)]
    pub require_dev: HashMap<String, String>,

    /// Download information for the distribution archive, if one is published.
    pub dist: Option<P2Dist>,

    /// VCS source reference, if available (used when installing from source).
    pub source: Option<P2Source>,

    /// Composer package type (e.g. `"library"`, `"project"`, `"composer-plugin"`).
    #[serde(rename = "type")]
    pub package_type: Option<String>,

    /// Short description of the package.
    pub description: Option<String>,

    /// Project homepage URL.
    pub homepage: Option<String>,

    /// Release timestamp in ISO 8601 format.
    pub time: Option<String>,

    /// URL that Composer pings after a successful install (download tracking).
    #[serde(rename = "notification-url")]
    pub notification_url: Option<String>,

    /// SPDX license identifier(s). Can be a string or an array of strings.
    #[serde(default)]
    pub license: Value,

    /// Arbitrary keywords describing the package.
    #[serde(default)]
    pub keywords: Vec<String>,

    /// Package authors.
    #[serde(default)]
    pub authors: Vec<P2Author>,

    /// Support links (issues, source, docs, etc.).
    #[serde(default)]
    pub support: HashMap<String, String>,

    /// Arbitrary extra metadata preserved from composer.json.
    pub extra: Option<Value>,

    /// Autoload rules: keys are autoload types (`"psr-4"`, `"classmap"`, etc.).
    #[serde(default)]
    pub autoload: HashMap<String, Value>,

    /// Packages that this version replaces: package name to version constraint.
    #[serde(default)]
    pub replace: HashMap<String, String>,

    /// Virtual packages that this version provides: package name to version.
    #[serde(default)]
    pub provide: HashMap<String, String>,

    /// Executable scripts shipped with this package, relative to the package root.
    #[serde(default)]
    pub bin: Vec<String>,
}

/// An author entry inside a p2 version.
#[derive(Debug, Deserialize, Clone)]
pub struct P2Author {
    /// Author's display name.
    pub name: Option<String>,
    /// Author's email address.
    pub email: Option<String>,
    /// Author's personal or project homepage URL.
    pub homepage: Option<String>,
    /// Author's role in the project (e.g. `"maintainer"`, `"contributor"`).
    pub role: Option<String>,
}

/// VCS source information for installing a package from source.
#[derive(Debug, Deserialize, Clone)]
pub struct P2Source {
    /// VCS backend type.
    #[serde(rename = "type")]
    pub vcs_type: VcsType,
    /// Repository URL.
    pub url: String,
    /// Commit hash or tag identifying the exact revision.
    pub reference: String,
}

/// Distribution archive download information.
#[derive(Debug, Deserialize, Clone)]
pub struct P2Dist {
    /// Archive format.
    #[serde(rename = "type")]
    pub dist_type: DistType,
    /// Download URL for the archive.
    pub url: String,
    /// Commit hash or tag this archive was built from.
    pub reference: Option<String>,
    /// SHA-1 checksum of the downloaded archive, as published by Packagist.
    ///
    /// SHA-1 is weak against collision attacks, but it is what Packagist publishes
    /// and Phast verifies it post-download as an integrity guard against corruption
    /// and accidental cache poisoning. Verified in the downloader before extraction.
    pub shasum: Option<String>,
}

/// Expand a raw p2 version array into fully-merged `P2Version` objects.
///
/// When `minified` is set, each entry in the array inherits all fields from the
/// previous one. Merging happens on raw `serde_json::Value` objects before
/// deserializing, so no field is lost on the way through.
///
/// An entry that does not deserialize into a `P2Version` is dropped without a
/// word, so the returned list can be shorter than `raw_versions`.
pub fn expand_versions(raw_versions: Vec<Value>, minified: bool) -> Vec<P2Version> {
    if !minified || raw_versions.is_empty() {
        return raw_versions
            .into_iter()
            .filter_map(|v| from_value(v).ok())
            .collect();
    }

    let mut versions: Vec<P2Version> = Vec::with_capacity(raw_versions.len());
    let mut snapshot = Object(Map::new());

    for entry in raw_versions {
        if let (Object(delta), Object(map)) = (entry, &mut snapshot) {
            apply_delta(map, delta);
        }
        if let Ok(version) = P2Version::deserialize(&snapshot) {
            versions.push(version);
        }
    }
    versions
}

/// Merge a minified-format delta into the running state map.
///
/// Each key in `delta` either updates `snapshot` (normal value) or removes a
/// field from it (`"__unset"` sentinel). After applying all deltas in order,
/// `snapshot` holds the complete state for the current version.
fn apply_delta(snapshot: &mut Map<String, Value>, delta: Map<String, Value>) {
    for (k, v) in delta {
        // Composer minified-format sentinel: https://getcomposer.org/doc/04-schema.md
        if v.as_str() == Some("__unset") {
            snapshot.remove(&k);
        } else {
            snapshot.insert(k, v);
        }
    }
}
