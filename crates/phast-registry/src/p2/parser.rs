//! Parsing registry metadata responses into a `ParsedPackage`.

use crate::p2::{P2Response, P2Version, expand_versions};

use phast_core::package::PackageName;
use phast_version::Version;
use phast_version::parser;

use anyhow::Context;
use serde_json::Value;
use serde_json::from_value;

/// A package's metadata response, parsed and expanded for the resolver.
pub struct ParsedPackage {
    /// Every version the resolver may select, paired with the raw p2 entry it came
    /// from. A branch carrying a `branch-alias` appears twice, once under each name,
    /// so the count here is not the number of releases the registry published.
    pub versions: Vec<(Version, P2Version)>,
}

#[rustfmt::skip]
impl ParsedPackage {
    /// Parse a p2 (Composer v2) metadata response for `name`.
    ///
    /// Expands the minified Composer 2.0 delta format if present, then turns each
    /// entry into its version(s). Unparseable entries are skipped.
    ///
    /// A response carrying no entry for `name` parses to no versions rather than an
    /// error, so a registry answering with the wrong package reads as an empty one.
    pub fn from_p2_response(name: &PackageName, json: &str) -> anyhow::Result<Self> {
        let mut response: P2Response = serde_json::from_str(json)
            .with_context(|| format!("failed to parse p2 metadata for '{}'", name.as_str()))?;
        let minified = response.minified.as_deref() == Some("composer/2.0");

        // Use remove to take ownership without cloning; look up by name so a response
        // that contains an unexpected key (registry bug, wrong package) does not
        // silently return the wrong version list.
        let raw_versions = response
            .packages
            .remove(name.as_str())
            .unwrap_or_default();

        let versions = expand_versions(raw_versions, minified)
            .into_iter()
            .flat_map(expand_version)
            .collect();

        Ok(Self { versions })
    }

    /// Parse a v1 (Satis inline) package entry, a JSON object keyed by version string
    /// (`{"1.0.0": {...}, "2.0.0": {...}}`).
    ///
    /// `None` when the value is not an object. An entry inside one that does not read
    /// as a version is dropped, and the rest are kept.
    pub fn from_v1(json: &Value) -> Option<Self> {
        let versions = json
            .as_object()?
            .values()
            .filter_map(|v| from_value::<P2Version>(v.clone()).ok())
            .flat_map(expand_version)
            .collect();

        Some(Self { versions })
    }
}

/// Turn one p2 version entry into its resolver version(s).
///
/// A `dev-<branch>` aliased in `extra.branch-alias` (e.g. `dev-main` -> `2.9-dev`)
/// is offered as *both* the branch and that numeric dev version, so a numeric
/// require like `^2.9` can select the branch, matching Composer. Unparseable
/// entries yield nothing.
fn expand_version(p2v: P2Version) -> Vec<(Version, P2Version)> {
    let version = p2v
        .version_normalized
        .as_deref()
        .or(p2v.version.as_deref())
        .unwrap_or_default();

    // Skip entries whose version string we can't parse.
    let Ok(version) = parser::parse_version(version) else {
        return Vec::new();
    };

    let mut versions = Vec::with_capacity(2);

    // Also offer a branch under its numeric branch-alias,
    // so numeric requires can select it.
    if let Some(alias) = branch_alias_for(&version, &p2v) {
        versions.push((alias, p2v.clone()));
    }

    // Always offer the version as published.
    versions.push((version, p2v));
    versions
}

/// The numeric dev version a `dev-<branch>` presents as via its own
/// `extra.branch-alias` entry (`"dev-<branch>" -> "<target>"`), if any.
#[rustfmt::skip]
fn branch_alias_for(version: &Version, p2v: &P2Version) -> Option<Version> {
    let Version::Branch(branch) = version else {
        return None;
    };

    let target = p2v
        .extra
        .as_ref()?
        .get("branch-alias")?
        .get(format!("dev-{branch}"))?
        .as_str()?;
    parser::parse_branch_alias(target)
        .ok()
        .map(Version::Numeric)
}
