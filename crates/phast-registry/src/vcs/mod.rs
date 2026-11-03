//! VCS repositories: installing a package straight from a git host.
//!
//! A [`VcsHost`] driver turns a hosted git repository into what the resolver needs,
//! entirely over the host's HTTP API: no local git, and no clone.
//!
//! Adding a host is additive, a new [`VcsHost`] impl plus an arm in [`Host`] and in
//! [`host_for`]. The [`VcsProvider`](crate::provider::VcsProvider) built on top is
//! host-agnostic, so a new host never touches it, the router, or the resolver.

pub mod candidate;
pub mod github;

pub use candidate::Candidate;
pub use github::GitHubHost;

use crate::http::client::HttpClient;

use std::future::Future;

use phast_core::lock::phast::DistSection;
use phast_core::manifest::Manifest;
use phast_version::Version;
use phast_version::parser::parse_version;

use anyhow::{Context, Result, bail};
use reqwest::Url;

/// A tag or branch in a repository, with the commit it points at.
#[derive(Clone)]
pub struct VcsRef {
    /// Ref name: a tag like `v1.2.0` or a branch like `main`.
    pub name: String,
    /// Commit the ref resolves to.
    pub commit: String,
    /// Whether this ref is a tag (a release) or a branch (a `dev-*` version).
    pub kind: RefKind,
}

impl VcsRef {
    /// The version this ref represents: a tag's numeric version (leading
    /// `v` stripped, unparseable tags dropped), a branch's `dev-*` version,
    /// or `None` for a commit.
    pub fn version(&self) -> Option<Version> {
        match self.kind {
            RefKind::Tag => parse_version(self.name.trim_start_matches('v')).ok(),
            RefKind::Branch => Some(Version::Branch(self.name.clone())),
            RefKind::Commit => None,
        }
    }
}

/// Whether a [`VcsRef`] is a release tag, a development branch, or a bare commit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefKind {
    /// A tag, surfaced as a numeric version.
    Tag,
    /// A branch, surfaced as a `dev-*` version.
    Branch,
    /// An exact commit, surfacing no version: the manifest at it supplies one.
    Commit,
}

/// What every git host driver provides, over the host's HTTP API.
///
/// Implementors: [`GitHubHost`], and [`Host`], which dispatches to it.
///
/// Async methods return `Send` futures to match [`Provider`](phast_resolver::provider::Provider),
/// so a [`VcsProvider`](crate::provider::VcsProvider) over a host is usable across the resolver's threads.
#[rustfmt::skip]
pub trait VcsHost: Send + Sync {
    /// All tags and branches in the repository, with their commits.
    fn refs(&self) -> impl Future<Output = Result<Vec<VcsRef>>> + Send;

    /// The repository's default branch, used by a `[sources]` pin that names no ref.
    fn branch(&self) -> impl Future<Output = Result<String>> + Send;

    /// When `reference`'s commit landed, as an RFC 3339 timestamp.
    ///
    /// The lock's `time` for a git-sourced package, where a registry would supply it
    /// from its metadata. `None` when the host reports no date.
    fn committed_at(
        &self,
        reference: &str,
    ) -> impl Future<Output = Result<Option<String>>> + Send;

    /// Raw bytes of `path` at `reference` (a tag, branch, or commit),
    /// or `None` when the file does not exist there.
    ///
    /// The primitive every host implements. Manifest resolution ([`manifest_at`])
    /// builds on it, so which manifest file wins is decided in one place rather
    /// than once per host.
    fn file(
        &self,
        reference: &str,
        path: &str,
    ) -> impl Future<Output = Result<Option<Vec<u8>>>> + Send;

    /// A downloadable archive (the host's zipball endpoint) for `reference`.
    fn dist(&self, reference: &str) -> DistSection;
}

/// Dispatch over the supported git hosts.
///
/// [`VcsHost`] is an RPITIT trait and so not `dyn`-compatible, the same reason
/// [`RoutedProvider`](crate::router::RoutedProvider) is an enum;
/// `Host` delegates to its variant. New hosts add a variant here.
#[derive(Clone)]
pub enum Host {
    /// A repository on github.com.
    GitHub(GitHubHost),
}

#[rustfmt::skip]
impl VcsHost for Host {
    async fn refs(&self) -> Result<Vec<VcsRef>> {
        match self { Host::GitHub(host) => host.refs().await }
    }

    async fn branch(&self) -> Result<String> {
        match self { Host::GitHub(host) => host.branch().await }
    }

    async fn committed_at(&self, reference: &str) -> Result<Option<String>> {
        match self { Host::GitHub(host) => host.committed_at(reference).await }
    }

    async fn file(&self, reference: &str, path: &str) -> Result<Option<Vec<u8>>> {
        match self { Host::GitHub(host) => host.file(reference, path).await }
    }

    fn dist(&self, reference: &str) -> DistSection {
        match self { Host::GitHub(host) => host.dist(reference) }
    }
}

/// Pick a host driver for a repository URL, or `None` if the host is unsupported.
///
/// A URL that does not parse, names no host, or names a host with no driver yields
/// `None`, which covers self-hosted git and SSH remotes as much as a host nobody
/// has written a driver for.
#[rustfmt::skip]
pub fn host_for(url: &str, client: HttpClient) -> Option<Host> {
    let host = Url::parse(url)
        .ok()?
        .host_str()?
        .to_owned();

    match host.as_str() {
        "github.com" => GitHubHost::parse(url, client).map(Host::GitHub),
                   _ => None,
    }
}

/// The package manifest at `reference`, trying `composer.json` then `phast.toml`.
///
/// `composer.json` is checked first and wins; `phast.toml` is the fallback. This
/// is shared across every host so a Composer repo and a Phast-native repo both
/// resolve correctly. Errors when neither file exists.
#[rustfmt::skip]
pub async fn manifest_at(host: &impl VcsHost, reference: &str) -> Result<Manifest> {
    if let Some(bytes) = host.file(reference, "composer.json").await? {
        let text = String::from_utf8(bytes)
            .context("composer.json is not valid UTF-8")?;
        return Manifest::from_composer_json(&text)
            .map_err(|e| anyhow::anyhow!("{e}"));
    }

    if let Some(bytes) = host.file(reference, "phast.toml").await? {
        let text = String::from_utf8(bytes)
            .context("phast.toml is not valid UTF-8")?;
        return Manifest::from_phast_toml(&text)
            .map_err(|e| anyhow::anyhow!("{e}"));
    }

    bail!("neither composer.json nor phast.toml is present at {reference}")
}
