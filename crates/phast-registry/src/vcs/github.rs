//! GitHub host driver: refs, per-ref file reads, and zipballs, with no local git.
//!
//! Two hosts are involved, which is why the driver holds both bases: refs, commit
//! dates and zipballs come from the REST API, while a file at a ref is read from the
//! raw-content host instead.

use crate::http::client::HttpClient;
use crate::vcs::{RefKind, VcsHost, VcsRef};

use phast_core::lock::phast::{DistSection, DistType};

use anyhow::{Context, Result};
use chrono::DateTime;
use reqwest::{StatusCode, Url, header};
use serde::Deserialize;

/// A repository on github.com, addressed through the GitHub REST API.
#[derive(Clone)]
pub struct GitHubHost {
    /// Repository owner (the `acme` in `github.com/acme/widget`).
    owner: String,
    /// Repository name (the `widget` in `github.com/acme/widget`).
    repo: String,
    /// Shared HTTP client; carries the `github-token` credential for `github.com`.
    http_client: HttpClient,
    /// API base, e.g. `https://api.github.com`.
    api_base: String,
    /// Raw-content base, e.g. `https://raw.githubusercontent.com`.
    raw_base: String,
}

#[rustfmt::skip]
impl GitHubHost {
    const API_BASE: &str = "https://api.github.com";
    const RAW_BASE: &str = "https://raw.githubusercontent.com";

    /// Build a driver from explicit `owner`/`repo` and base URLs.
    ///
    /// The general form, taking both bases so they can point somewhere other than
    /// github.com. [`parse`](Self::parse) is the one that derives them from a URL.
    pub fn new(
        owner: impl Into<String>,
        repo: impl Into<String>,
        client: HttpClient,
        api_base: impl Into<String>,
        raw_base: impl Into<String>,
    ) -> Self {
        Self {
            owner: owner.into(),
            repo: repo.into(),
            http_client: client,
            api_base: api_base.into(),
            raw_base: raw_base.into(),
        }
    }

    /// Parse `https://github.com/{owner}/{repo}` (optionally `.git`) into a driver.
    ///
    /// Returns `None` for non-github.com URLs or URLs missing owner/repo.
    pub fn parse(url: &str, client: HttpClient) -> Option<Self> {
        let parsed = Url::parse(url).ok()?;
        if parsed.host_str()? != "github.com" {
            return None;
        }

        let mut segments = parsed
            .path_segments()?;
        let owner = segments
            .next()?;
        let repo = segments
            .next()?
            .trim_end_matches(".git");
        if owner.is_empty() || repo.is_empty() {
            return None;
        }

        Some(Self::new(
            owner,
            repo,
            client,
            Self::API_BASE,
            Self::RAW_BASE,
        ))
    }

    /// Fetch one ref listing endpoint (`tags` or `branches`) and tag each entry.
    async fn list_refs(&self, endpoint: &str, kind: RefKind) -> Result<Vec<VcsRef>> {
        let url = format!(
            "{}/repos/{}/{}/{endpoint}",
            self.api_base, self.owner, self.repo
        );

        let response: reqwest::Response = self
            .http_client
            .get(&url)?
            .header(header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .context("failed to reach GitHub API")?
            .error_for_status()
            .with_context(|| format!("failed to list {endpoint} for {}/{}", self.owner, self.repo))?;

        let data = response
            .json::<Vec<GitHubRef>>()
            .await
            .context("failed to parse GitHub API response")?;

        Ok(data
            .into_iter()
            .map(|item| VcsRef {
                name: item.name,
                commit: item.commit.sha,
                kind,
            })
            .collect())
    }
}

#[rustfmt::skip]
impl VcsHost for GitHubHost {
    /// Tags first, then branches, all in one list.
    async fn refs(&self) -> Result<Vec<VcsRef>> {
        let tags = self
            .list_refs("tags", RefKind::Tag)
            .await?;
        let branches = self
            .list_refs("branches", RefKind::Branch)
            .await?;

        Ok(tags
            .into_iter()
            .chain(branches)
            .collect(),
        )
    }

    /// The repository's `default_branch`, from the repository endpoint.
    async fn branch(&self) -> Result<String> {
        let url = format!(
            "{}/repos/{}/{}",
            self.api_base,
            self.owner,
            self.repo,
        );

        let response: reqwest::Response = self
            .http_client
            .get(&url)?
            .header(header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .context("failed to reach GitHub API")?
            .error_for_status()
            .with_context(|| format!("failed to read {}/{}", self.owner, self.repo))?;

        let data = response
            .json::<GitHubRepo>()
            .await
            .context("failed to parse GitHub API response")?;

        Ok(data.default_branch)
    }

    /// The commit date for `reference`, normalized to the offset form the lock uses.
    ///
    /// Takes the *committer* date, not the author's: a rebased or cherry-picked patch
    /// keeps its original author date, which can predate the release by a long way,
    /// while the committer date is when the code actually landed on the ref.
    ///
    /// A date that does not parse yields `None`, the same answer as a commit GitHub
    /// reports no date for, since neither is worth failing a lock over.
    async fn committed_at(&self, reference: &str) -> Result<Option<String>> {
        let url = format!(
            "{}/repos/{}/{}/commits/{reference}",
            self.api_base,
            self.owner,
            self.repo,
        );

        let response: reqwest::Response = self
            .http_client
            .get(&url)?
            .header(header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .context("failed to reach GitHub API")?
            .error_for_status()
            .with_context(|| format!("failed to read commit {reference} in {}/{}", self.owner, self.repo))?;

        let date = response
            .json::<GitHubCommitDetail>()
            .await
            .context("failed to parse GitHub API response")?
            .commit
            .committer
            .date;

        // GitHub sends `...Z`; the lock records the `+00:00` form registries use,
        // so a git-sourced entry and a registry-sourced one read the same.
        Ok(DateTime::parse_from_rfc3339(&date)
            .map(|parsed| parsed.to_rfc3339())
            .ok())
    }

    /// Read `path` at `reference` from the raw-content host; `None` on 404.
    async fn file(&self, reference: &str, path: &str) -> Result<Option<Vec<u8>>> {
        let url = format!(
            "{}/{}/{}/{reference}/{path}",
            self.raw_base,
            self.owner,
            self.repo,
        );

        let response: reqwest::Response = self
            .http_client
            .get(&url)?
            .send()
            .await
            .with_context(|| format!("failed to fetch {}", url))?;

        // A missing file is the expected "not present at this ref" signal,
        // not an error.
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let bytes: bytes::Bytes = response
            .error_for_status()
            .with_context(|| format!("failed to fetch {path} at {reference} in {}/{}", self.owner, self.repo))?
            .bytes()
            .await
            .with_context(|| format!("failed to read response from {}", url))?;

        Ok(Some(bytes.to_vec()))
    }

    /// The GitHub zipball for `reference`.
    ///
    /// No `shasum` accompanies it, because GitHub publishes none for a zipball,
    /// so the archive it yields is accepted without a checksum comparison.
    fn dist(&self, reference: &str) -> DistSection {
        DistSection {
            dist_type: DistType::Zip,
            url: format!(
                "{}/repos/{}/{}/zipball/{reference}",
                self.api_base,
                self.owner,
                self.repo,
            ),
            reference: Some(reference.to_owned()),
            shasum: None,
        }
    }
}

/// One entry from GitHub's `/tags` or `/branches` listing.
#[derive(Deserialize)]
struct GitHubRef {
    name: String,
    commit: GitHubCommit,
}

#[derive(Deserialize)]
struct GitHubCommit {
    sha: String,
}

/// The commit object from GitHub's `/repos/{owner}/{repo}/commits/{ref}`.
#[derive(Deserialize)]
struct GitHubCommitDetail {
    commit: GitHubCommitMeta,
}

#[derive(Deserialize)]
struct GitHubCommitMeta {
    committer: GitHubSignature,
}

#[derive(Deserialize)]
struct GitHubSignature {
    date: String,
}

/// The repository object from GitHub's `/repos/{owner}/{repo}`.
#[derive(Deserialize)]
struct GitHubRepo {
    default_branch: String,
}
