//! Advisory provider backed by the Packagist security advisory API
//! (`https://packagist.org/api/security-advisories/`).

use super::advisory::{
    Advisory, AdvisoryProvider, Severity, is_version_affected, parse_reported_at,
};

use std::collections::HashMap;

use phast_registry::http::client::HttpClient;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Fetches Packagist advisories in batches, then filters them against the locked versions.
pub struct PackagistAdvisoryProvider {
    base_url: String,
    http_client: HttpClient,
}

#[rustfmt::skip]
impl PackagistAdvisoryProvider {
    const BASE_URL:      &'static str = "https://packagist.org/api";
    const ADVISORY_PATH: &'static str = "/security-advisories/";
    /// Max packages per GET request to stay well under common URL length limits.
    const BATCH_SIZE: usize = 50;

    pub fn new(client: HttpClient) -> Self {
        Self {
            http_client: client,
            base_url: Self::BASE_URL.to_string(),
        }
    }
}

/// Top-level response from `GET /api/security-advisories/`.
#[derive(Deserialize)]
struct PackagistResponse {
    /// Map of `vendor/package` to the list of advisories for that package.
    advisories: HashMap<String, Vec<PackagistRawAdvisory>>,
}

/// One advisory entry as returned by the Packagist advisory API.
#[derive(Deserialize)]
struct PackagistRawAdvisory {
    /// Short title of the vulnerability.
    title: String,
    /// URL to the full advisory (GitHub Security Advisory or FriendsOfPHP).
    link: String,
    /// CVE identifier, if assigned.
    cve: Option<String>,
    /// Severity label from Packagist (`low`, `medium`, `high`, `critical`).
    severity: Option<String>,
    /// Remote advisory ID (e.g. GHSA-*) used as fallback when CVE is absent.
    #[serde(rename = "remoteId")]
    remote_id: Option<String>,
    /// Composer constraint describing which versions are affected.
    #[serde(rename = "affectedVersions")]
    affected_versions: String,
    /// ISO 8601 timestamp when the advisory was reported to Packagist.
    #[serde(rename = "reportedAt")]
    reported_at: Option<String>,
}

#[rustfmt::skip]
impl AdvisoryProvider for PackagistAdvisoryProvider {
    /// Query Packagist in batches, then keep only the advisories matching each locked
    /// version.
    ///
    /// The request carries package names and no versions, so the API returns every
    /// advisory a package has ever had and the version filtering happens here. The OSV
    /// provider is the other way round, which is why only this one calls
    /// `is_version_affected`, and why that function errs toward keeping an advisory it
    /// cannot parse.
    async fn query(&self, packages: &[(String, String)]) -> Result<Vec<Advisory>> {
        // Packagist advisory API only accepts GET. To avoid 414 URI Too Long on
        // large lock files, split packages into batches and merge the responses.
        let mut advisories: HashMap<String, Vec<PackagistRawAdvisory>> = HashMap::new();

        for chunk in packages.chunks(Self::BATCH_SIZE) {
            let query = chunk
                .iter()
                .map(|(name, _)| format!("packages%5B%5D={}", name.replace('/', "%2F")))
                .collect::<Vec<_>>()
                .join("&");

            let url = format!("{}{}?{}", self.base_url, Self::ADVISORY_PATH, query);

            let response = self
                .http_client
                .get(&url)?
                .send()
                .await
                .context("failed to reach Packagist advisory API")?
                .error_for_status()
                .with_context(|| format!("Packagist advisory API at {} returned an error", url))?;

            let data = response
                .json::<PackagistResponse>()
                .await
                .context("failed to parse Packagist advisory response")?;

            for (package, advisory) in data.advisories {
                advisories
                    .entry(package)
                    .or_default()
                    .extend(advisory);
            }
        }

        if advisories.is_empty() {
            return Ok(Vec::new());
        }

        let data = PackagistResponse { advisories };

        let version_by_name: HashMap<&str, &str> = packages
            .iter()
            .map(|(n, v)| (n.as_str(), v.as_str()))
            .collect();

        let mut advisories = Vec::new();
        for (name, raw) in data.advisories {
            let locked = version_by_name
                .get(name.as_str())
                .copied()
                .unwrap_or("");
            for advisory in raw {
                if !is_version_affected(locked, &advisory.affected_versions) {
                    continue;
                }

                let advisory_id = advisory
                    .cve
                    .or(advisory.remote_id)
                    .unwrap_or_else(|| "UNKNOWN".to_string());

                let advisory_severity = advisory
                    .severity
                    .as_deref()
                    .map(Severity::from_label)
                    .unwrap_or(Severity::Unknown);

                let reported_at = advisory
                    .reported_at
                    .as_deref()
                    .and_then(parse_reported_at);

                advisories.push(Advisory {
                    package:           name.clone(),
                    version:           locked.to_string(),
                    id:                advisory_id,
                    title:             advisory.title,
                    severity:          advisory_severity,
                    url:               advisory.link,
                    affected_versions: advisory.affected_versions,
                    reported_at,
                });
            }
        }

        Ok(advisories)
    }
}
