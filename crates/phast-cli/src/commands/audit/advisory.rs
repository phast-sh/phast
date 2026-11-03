//! The normalized advisory shape both providers report, and the rules that decide
//! whether a locked version is actually affected.

use phast_resolver::ranges::constraint_to_range;
use phast_version::{Constraint, Version};

use anyhow::Result;
use chrono::{DateTime, NaiveDate, NaiveDateTime};
use serde::Serialize;

/// A normalized security advisory, independent of the source provider.
#[derive(Debug, Clone, Serialize)]
pub struct Advisory {
    /// The `vendor/package` name of the affected package.
    pub package: String,
    /// The locked version that triggered this advisory.
    pub version: String,
    /// Advisory identifier, e.g. `CVE-2022-31091` or `GHSA-q559-8m2m-g699`.
    pub id: String,
    /// Short human-readable title of the vulnerability.
    pub title: String,
    /// Severity level of the vulnerability.
    pub severity: Severity,
    /// URL to the full advisory details.
    pub url: String,
    /// The version constraint describing which versions are affected.
    pub affected_versions: String,
    /// Date the advisory was published to the database, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reported_at: Option<NaiveDate>,
}

/// Severity level of a security advisory.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
    Unknown,
}

#[rustfmt::skip]
impl Severity {
    /// Parse a severity label as returned by advisory providers.
    /// Accepts Packagist labels (`high`, `critical`, ...) and GitHub's
    /// `moderate` as an alias for `medium`. Case-insensitive.
    pub(super) fn from_label(string: &str) -> Self {
        match string.to_ascii_lowercase().as_str() {
            "low"                 => Self::Low,
            "medium" | "moderate" => Self::Medium,
            "high"                => Self::High,
            "critical"            => Self::Critical,
            _                     => Self::Unknown,
        }
    }

    /// Upper-case string label for text output.
    ///
    /// JSON does not use this: `rename_all` on the enum produces the same strings
    /// by coincidence, so changing one here will not change the other.
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            Self::Low      => "LOW",
            Self::Medium   => "MEDIUM",
            Self::High     => "HIGH",
            Self::Critical => "CRITICAL",
            Self::Unknown  => "UNKNOWN",
        }
    }
}

/// A source of security advisories that can be queried for a set of packages.
///
/// Implementors: `PackagistAdvisoryProvider`, `OsvAdvisoryProvider`, chosen by `--provider`.
///
/// Only used internally within phast-cli with concrete types, so no dyn dispatch is
/// needed and `async_fn_in_trait` is allowed.
#[allow(async_fn_in_trait)]
pub trait AdvisoryProvider {
    /// Query the provider for all advisories affecting the given locked packages.
    ///
    /// `packages` is a slice of `(vendor/package, version)` pairs taken
    /// directly from `phast.lock`. Returns only advisories that match the
    /// locked version; packages with no advisories are omitted.
    async fn query(&self, packages: &[(String, String)]) -> Result<Vec<Advisory>>;
}

/// Whether `locked_version` falls within the `affected_version` constraint.
///
/// `true` when the version is affected, and `true` conservatively when the constraint
/// cannot be parsed, so a potentially vulnerable package is never silently skipped.
/// `false` when either input is empty, or when the locked version cannot be parsed:
/// neither says anything about affectedness.
pub(super) fn is_version_affected(locked_version: &str, affected_version: &str) -> bool {
    // Nothing to check if either input is absent.
    if locked_version.is_empty() || affected_version.is_empty() {
        return false;
    }
    // FriendsOfPHP advisories use `=X.Y.Z` for exact version matches anywhere in
    // the constraint (e.g. `>=2.25.0,<2.25.2|=2.23.0|<2.18.1`). Strip the bare
    // `=` prefix from each segment since a plain version is already exact in Composer.
    let normalized = affected_version
        .split('|')
        .map(|seg| {
            seg.strip_prefix('=')
                .filter(|v| !v.starts_with('='))
                .unwrap_or(seg)
        })
        .collect::<Vec<_>>()
        .join("|");
    let affected_version = normalized.as_str();
    // Unparseable constraint: flag conservatively rather than silently skip.
    let Ok(constraint) = affected_version.parse::<Constraint>() else {
        return true;
    };
    // Unparseable locked version: cannot determine affectedness, skip safely.
    let Ok(version) = locked_version.parse::<Version>() else {
        return false;
    };
    constraint_to_range(&constraint).contains(&version)
}

/// Parse a date string from an advisory provider into a `NaiveDate`.
///
/// Handles both RFC 3339 (`2024-11-15T12:31:44+00:00`) and the Packagist
/// space-separated format (`2024-11-15 12:31:44`). Returns `None` on failure.
pub(super) fn parse_reported_at(date: &str) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(date)
        .map(|dt| dt.date_naive())
        .or_else(|_| NaiveDateTime::parse_from_str(date, "%Y-%m-%d %H:%M:%S").map(|dt| dt.date()))
        .ok()
}
