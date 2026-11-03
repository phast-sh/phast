//! Advisory provider backed by the OSV (Open Source Vulnerabilities) API.
//!
//! `POST /v1/querybatch` returns minimal refs (id + modified only); full details are
//! fetched concurrently from `GET /v1/vulns/{id}`.

use super::advisory::{Advisory, AdvisoryProvider, Severity, parse_reported_at};

use std::collections::HashMap;
use std::iter::zip;

use phast_registry::http::client::HttpClient;

use anyhow::{Context, Result};
use futures::future::join_all;
use serde::{Deserialize, Serialize};

/// Queries OSV for vulnerability IDs, then fetches each match's full record.
pub struct OsvAdvisoryProvider {
    base_url: String,
    http_client: HttpClient,
}

#[rustfmt::skip]
impl OsvAdvisoryProvider {
    const BASE_URL:   &'static str = "https://api.osv.dev";
    const QUERY_PATH: &'static str = "/v1/querybatch";
    const VULN_PATH:  &'static str = "/v1/vulns/";

    pub fn new(client: HttpClient) -> Self {
        Self {
            base_url: Self::BASE_URL.to_string(),
            http_client: client,
        }
    }
}

/// Request body for `POST /v1/querybatch`.
#[derive(Serialize)]
struct OsvBatchRequest<'a> {
    queries: Vec<OsvQuery<'a>>,
}

/// A single package query within an OSV batch request.
#[derive(Serialize)]
struct OsvQuery<'a> {
    /// Exact locked version to check.
    version: &'a str,
    package: OsvPackage<'a>,
}

/// Package identifier in an OSV query.
#[derive(Serialize)]
struct OsvPackage<'a> {
    name: &'a str,
    /// Always `"Packagist"` for PHP packages.
    ecosystem: &'static str,
}

/// Top-level response from `POST /v1/querybatch`.
///
/// Results come back in query order, which is what lets `query` pair `results[i]` with
/// `packages[i]`. Nothing checks it, so a reordering upstream would misattribute every
/// advisory rather than fail.
#[derive(Deserialize)]
struct OsvBatchResponse {
    results: Vec<OsvBatchResult>,
}

/// Vulnerabilities matching one query in the batch.
#[derive(Deserialize)]
struct OsvBatchResult {
    /// Minimal vulnerability references. Empty when no vulnerabilities match.
    #[serde(rename = "vulns", default)]
    vulnerabilities: Vec<OsvVulnerability>,
}

/// Minimal vulnerability reference returned by querybatch (id only).
/// Full details must be fetched separately from `GET /v1/vulns/{id}`.
#[derive(Deserialize)]
struct OsvVulnerability {
    id: String,
}

/// Full vulnerability record returned by `GET /v1/vulns/{id}`.
#[derive(Deserialize)]
struct OsvVulnerabilityDetail {
    /// GHSA or OSV identifier.
    id: String,
    /// One-line summary of the vulnerability.
    summary: Option<String>,
    /// ISO 8601 timestamp when the advisory was published.
    published: Option<String>,
    /// Affected ranges, used to extract the version constraint string.
    #[serde(default)]
    affected: Vec<OsvAffected>,
    /// Links to advisory pages, patches, and other resources.
    #[serde(default)]
    references: Vec<OsvReference>,
    /// Database-specific fields; GitHub Advisory Database populates `severity` here.
    database_specific: Option<OsvDbSpecific>,
}

/// Affected package entry in an OSV vulnerability record.
#[derive(Deserialize)]
struct OsvAffected {
    /// Version ranges describing affected versions.
    #[serde(default)]
    ranges: Vec<OsvRange>,
}

/// A single version range within an affected entry.
#[derive(Deserialize)]
struct OsvRange {
    /// Range type, e.g. `"ECOSYSTEM"`.
    #[serde(rename = "type")]
    range_type: String,
    /// Events marking introduced/fixed boundaries.
    #[serde(default)]
    events: Vec<OsvEvent>,
}

/// A single event (introduced or fixed) within a version range.
#[derive(Deserialize)]
struct OsvEvent {
    introduced: Option<String>,
    fixed: Option<String>,
}

/// A single reference URL in an OSV vulnerability record.
#[derive(Deserialize)]
struct OsvReference {
    /// Reference type, e.g. `"ADVISORY"`, `"FIX"`, `"WEB"`.
    #[serde(rename = "type")]
    ref_type: String,
    url: String,
}

/// Database-specific metadata embedded in an OSV vulnerability.
#[derive(Deserialize)]
struct OsvDbSpecific {
    /// Severity string as classified by the originating database
    /// (e.g. `"HIGH"`, `"MODERATE"` from GitHub Advisory Database).
    severity: Option<String>,
}

#[rustfmt::skip]
impl AdvisoryProvider for OsvAdvisoryProvider {
    /// Query OSV, in three phases over two round trips:
    ///
    /// 1. `POST /v1/querybatch` to discover which vulnerability IDs affect each package.
    /// 2. `GET /v1/vulns/{id}` for every matched ID (concurrently) to retrieve
    ///    the full record including summary and severity.
    /// 3. Invert the ID-to-packages map back into one advisory per affected package.
    async fn query(&self, packages: &[(String, String)]) -> Result<Vec<Advisory>> {
        // Step 1: querybatch to discover which vulnerability IDs affect each package.
        let queries: Vec<OsvQuery> = packages
            .iter()
            .map(|(name, version)| OsvQuery {
                version: version.as_str(),
                package: OsvPackage {
                    name: name.as_str(),
                    ecosystem: "Packagist",
                },
            })
            .collect();

        let url = format!("{}{}", self.base_url, Self::QUERY_PATH);
        let response = self
            .http_client
            .post(&url)?
            .json(&OsvBatchRequest { queries })
            .send()
            .await
            .context("failed to reach OSV API")?
            .error_for_status()
            .context("OSV API returned an error")?;

        let data: OsvBatchResponse = response
            .json()
            .await
            .context("failed to parse OSV querybatch response")?;
        if data.results.iter().all(|r| r.vulnerabilities.is_empty()) {
            return Ok(Vec::new());
        }

        // Map each vulnerability ID to the (package, version) pairs it was found in.
        let mut vulnerabilities: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for (i, result) in data
            .results
            .into_iter()
            .enumerate()
        {
            let (name, version) = &packages[i];
            for vulnerability in result.vulnerabilities {
                vulnerabilities
                    .entry(vulnerability.id)
                    .or_default()
                    .push((name.clone(), version.clone()));
            }
        }

        // Step 2: fetch full details for all unique IDs concurrently.
        // querybatch only returns id + modified; summary and severity require a
        // per-vuln GET to /v1/vulns/{id}.
        let vulnerability_ids: Vec<String> = vulnerabilities
            .keys()
            .cloned()
            .collect();

        let details = vulnerability_ids.iter().map(|id| {
            let url = format!("{}{}{id}", self.base_url, Self::VULN_PATH);
            async move {
                let response = self.http_client
                    .get(&url)?
                    .send()
                    .await
                    .and_then(|r| r.error_for_status())
                    .map_err(anyhow::Error::from)?;

                let data: OsvVulnerabilityDetail = response
                    .json()
                    .await
                    .map_err(anyhow::Error::from)?;

                Ok(data)
            }
        });
        let results: Vec<Result<OsvVulnerabilityDetail>> = join_all(details).await;

        // Step 3: build normalized advisories from the full details.
        let mut advisories = Vec::new();
        for (id, detail_result) in vulnerability_ids.iter().zip(results) {
            let detail = detail_result
                .with_context(|| format!("failed to fetch OSV vulnerability details for {id}"))?;

            let packages = vulnerabilities
                .get(id.as_str())
                .map(|v| v.as_slice())
                .unwrap_or(&[]);

            let advisory_severity = detail
                .database_specific
                .as_ref()
                .and_then(|d| d.severity.as_deref())
                .map(Severity::from_label)
                .unwrap_or(Severity::Unknown);

            let advisory_url = detail
                .references
                .iter()
                .find(|r| r.ref_type == "ADVISORY")
                .or_else(|| detail.references.first())
                .map(|r| r.url.clone())
                .unwrap_or_default();

            let advisory_title = detail
                .summary
                .unwrap_or_else(|| "(no description)".to_string());

            let reported_at = detail
                .published
                .as_deref()
                .and_then(parse_reported_at);

            let affected_versions = detail.affected.iter()
                .flat_map(|a| a.ranges.iter())
                .filter(|r| r.range_type == "ECOSYSTEM")
                .flat_map(|r| zip(
                    r.events.iter().filter_map(|e| e.introduced.as_deref()),
                    r.events.iter().filter_map(|e| e.fixed.as_deref()),
                )
                .map(|(i, f)| format!(">={},<{}", i, f)))
                .collect::<Vec<_>>()
                .join("|");

            for (name, version) in packages {
                advisories.push(Advisory {
                    package:           name.clone(),
                    version:           version.clone(),
                    id:                detail.id.clone(),
                    title:             advisory_title.clone(),
                    severity:          advisory_severity.clone(),
                    url:               advisory_url.clone(),
                    affected_versions: affected_versions.clone(),
                    reported_at,
                });
            }
        }

        Ok(advisories)
    }
}
