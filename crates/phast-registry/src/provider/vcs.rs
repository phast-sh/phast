//! [`VcsProvider`]: a package in a git repository, served over the
//! [`vcs`](crate::vcs) host layer with no local git.
//!
//! The difference between the constructors is the whole point:
//! [`discover`](VcsProvider::discover) offers every ref and lets the resolver
//! choose, while [`pin`](VcsProvider::pin) offers exactly the one ref a `[sources]`
//! entry named.

use crate::hints;
use crate::p2::convert::convert_extra;
use crate::provider::Lockable;
use crate::router::RegistryOptions;
use crate::vcs::{Candidate, Host, RefKind, VcsHost, VcsRef, manifest_at};

use phast_core::lock::phast::{AutoloadSection, PackageDef, SourceRef};
use phast_core::manifest::{GitRef, VcsType};
use phast_core::package::PackageName;
use phast_resolver::provider::{PackageMetadata, Provider};
use phast_version::Version;
use phast_version::parser::parse_declared_version;

use anyhow::Context;

/// A package resolved from a `vcs` repository on a supported host.
///
/// The VCS analogue of [`PathProvider`](super::PathProvider): it answers resolver queries
/// for the one package a repository contains, mapping the host's tags to versions and
/// branches to `dev-*`, reading each ref's manifest on demand. Host-agnostic, it holds
/// a [`Host`] and goes through the [`VcsHost`] contract, so a new host never touches it.
#[derive(Clone)]
pub struct VcsProvider {
    /// Declared package name, read from the default-branch manifest.
    name: PackageName,
    /// Original repository URL, recorded in the lock's `source`.
    url: String,
    /// Host driver.
    host: Host,
    /// The versions this provider offers, computed once at construction: from
    /// [`discover`](Self::discover) one per tag and branch plus the dev versions
    /// branches present as via `extra.branch-alias`, and from [`pin`](Self::pin)
    /// exactly one. A tag, a branch, and an alias are all uniform here.
    candidates: Vec<Candidate>,
}

#[rustfmt::skip]
impl VcsProvider {
    /// The package name this provider answers for.
    pub fn name(&self) -> &PackageName {
        &self.name
    }

    /// Build a provider for the repository at `url`, served by `host`.
    ///
    /// Lists the host's refs and reads the default branch's manifest to learn
    /// the package name (errors if it declares none) and any `extra.branch-alias`
    /// entries. `url` is recorded in the lock's `source`. The `minimum_stability` in
    /// `options` gates the dev-stability aliases and nothing else: tags and branches
    /// are offered whatever the floor, so a branch is only ever selected by a require
    /// that names it.
    ///
    /// The caller picks the `host` from the URL via [`host_for`](crate::vcs::host_for),
    /// so host selection (and the unsupported-host case) stays in the routing layer.
    pub async fn discover(url: &str, host: Host, options: RegistryOptions) -> anyhow::Result<Self> {
        let references = host
            .refs()
            .await?;

        let manifest = manifest_at(&host, "HEAD")
            .await?;

        let name = manifest
            .name
            .as_deref()
            .map(PackageName::new)
            .context("repository manifest declares no package name")?;

        let candidates = Candidate::all(
            &manifest,
            references,
            options.minimum_stability,
        );

        Ok(Self {
            name,
            url: url.to_owned(),
            host,
            candidates,
        })
    }

    /// Build a provider for a `[sources]` `{ url }` pin: one candidate, at `reference`.
    ///
    /// Where [`discover`](Self::discover) enumerates every ref and lets the resolver
    /// choose, a pin *is* the choice, so exactly one version is offered and the
    /// `[require]` constraint is checked against it (typically `"*"`). Exposing every
    /// ref here would make the two forms identical and erase the distinction.
    ///
    /// The version each ref shape surfaces as:
    ///
    /// - a branch, and a pin with no ref (the repository's default branch), as `dev-{branch}`;
    /// - a tag, as that tag's version;
    /// - a commit, as the version its manifest declares.
    ///
    /// `expected` is the `[sources]` key. The package name is therefore already known,
    /// so the manifest's own name is verified against it rather than read from it: a
    /// mismatch means the pin would install something other than what was asked for,
    /// and fails naming both.
    pub async fn pin(
        url: &str,
        host: Host,
        reference: &GitRef,
        expected: &PackageName,
    ) -> anyhow::Result<Self> {
        let reference = Self::reference_for(&host, reference)
            .await?;
        let manifest = manifest_at(&host, &reference.commit)
            .await?;

        let declared = manifest
            .name
            .as_deref()
            .map(PackageName::new)
            .context("repository manifest declares no package name")?;

        if &declared != expected {
            anyhow::bail!(
                "[sources] '{}' pins {url}, but its manifest declares '{}'",
                expected.as_str(),
                declared.as_str(),
            );
        }

        // A commit names no version of its own, so the manifest at that commit
        // supplies it; a tag or branch already carries one.
        let version = match reference.version() {
            Some(version) => version,
            None => {
                let declared = manifest
                    .version
                    .as_deref()
                    .with_context(|| format!(
                        "'{}' is pinned to commit {}, whose manifest declares no version",
                        expected.as_str(),
                        reference.commit,
                    ))?;
                parse_declared_version(declared)
                    .with_context(|| format!(
                        "invalid version '{declared}' for pinned package '{}'",
                        expected.as_str(),
                    ))?
            },
        };

        Ok(Self {
            name: expected.clone(),
            url: url.to_owned(),
            host,
            candidates: vec![Candidate::new(version, reference).with_manifest(manifest)],
        })
    }

    /// The locked entry for `version`: a zipball dist, the git source, and the
    /// manifest's require/autoload at that ref.
    ///
    /// `time` holds the commit's date when the host reports one, and is left out
    /// when it does not, since an install is not worth losing over a timestamp.
    async fn locked(&self, version: &Version) -> anyhow::Result<PackageDef> {
        let candidate = self.candidate_for(version)?;
        let reference = &candidate.reference;
        let manifest  = candidate
            .manifest(&self.host)
            .await?;

        // The ref's own version: a tag number, or `dev-{branch}` for a branch
        // and for an aliased branch (not the alias's sentinel-expanded numeric).
        let version = reference
            .version()
            .unwrap_or_else(|| version.clone())
            .to_string();

        let require = manifest
            .require
            .iter()
            .map(|(name, constraint)| (name.as_str().to_owned(), constraint.to_string()))
            .collect();

        let source = SourceRef {
            vcs_type: VcsType::Git,
            url: self.url.clone(),
            reference: reference.commit.clone(),
        };

        let autoload = AutoloadSection {
            psr4: manifest.autoload.psr4.clone(),
            psr0: manifest.autoload.psr0.clone(),
            classmap: manifest.autoload.classmap.clone(),
            files: manifest.autoload.files.clone(),
            exclude: manifest.autoload.exclude.clone(),
        };

        // A repository manifest carries no publish date, so the host supplies the
        // commit's, the way a registry supplies `time` from its metadata. A failure
        // here does not fail the lock, an install is not worth losing over a timestamp.
        let time = self
            .host
            .committed_at(&reference.commit)
            .await
            .ok()
            .flatten();

        Ok(PackageDef {
            name: self.name.as_str().to_owned(),
            version,
            require,
            dist: Some(self.host.dist(&reference.commit)),
            source: Some(source),
            package_type: manifest.package_type.clone(),
            description: manifest.description.clone(),
            time,
            license: manifest.license.clone().into_iter().collect(),
            autoload,
            extra: convert_extra(manifest.extra.clone()),
            ..Default::default()
        })
    }

    /// The candidate a resolved `version` corresponds to.
    fn candidate_for(&self, version: &Version) -> anyhow::Result<&Candidate> {
        self.candidates
            .iter()
            .find(|candidate| &candidate.version == version)
            .with_context(|| format!("no ref of {} matches {version}", self.name.as_str()))
    }

    /// Resolve a pinned [`GitRef`] to the repository ref it names.
    ///
    /// A commit is already a commit and needs no lookup. A branch or tag is matched
    /// against the host's ref listing so the lock records the commit it resolved to,
    /// not a moving name; an unknown ref fails naming what was looked for.
    async fn reference_for(host: &Host, reference: &GitRef) -> anyhow::Result<VcsRef> {
        // A pinned commit is the one shape needing no ref listing at all.
        if let GitRef::Reference(commit) = reference {
            return Ok(VcsRef {
                name: commit.clone(),
                commit: commit.clone(),
                kind: RefKind::Commit,
            });
        }

        let (name, kind) = match reference {
            GitRef::Branch(branch) => (branch.clone(), RefKind::Branch),
            GitRef::Tag(tag)       => (tag.clone(), RefKind::Tag),
            GitRef::Default        => (host.branch().await?, RefKind::Branch),
            GitRef::Reference(_)   => unreachable!("commit refs return early"),
        };

        host.refs()
            .await?
            .into_iter()
            .find(|reference| reference.kind == kind && reference.name == name)
            .with_context(|| format!("repository has no {} named '{name}'", match kind {
                RefKind::Tag => "tag",
                           _ => "branch",
            }))
    }
}

/// Answers from the candidates computed at construction, reading a ref's manifest
/// only when metadata is asked for.
///
/// A git package is pinned in the routing table under the name it declares, so it
/// never joins the fallback chain and is only ever asked about itself.
#[rustfmt::skip]
impl Provider for VcsProvider {
    async fn versions(&self, name: &PackageName) -> anyhow::Result<Vec<Version>> {
        if name != &self.name { return Ok(vec![]); }

        Ok(self
            .candidates
            .iter()
            .map(|candidate| candidate.version.clone())
            .collect(),
        )
    }

    async fn metadata(
        &self,
        name: &PackageName,
        version: &Version,
    ) -> anyhow::Result<PackageMetadata> {
        let candidate = self.candidate_for(version)?;
        let manifest  = candidate
            .manifest(&self.host)
            .await?;

        // Resolve the ref's require/replace/provide specs against the resolved version,
        // so a git package's declarations are honored exactly like a registry one's.

        let require = manifest
            .require
            .iter()
            .map(|(n, s)| (n.clone(), s.resolve(version)))
            .collect();
        let replace = manifest
            .replace
            .iter()
            .map(|(n, s)| (n.clone(), s.resolve(version)))
            .collect();
        let provide = manifest
            .provide
            .iter()
            .map(|(n, s)| (n.clone(), s.resolve(version)))
            .collect();

        Ok(PackageMetadata {
            name: name.clone(),
            version: version.clone(),
            require,
            require_dev: vec![],
            replace,
            provide,
        })
    }
}

/// The one leaf whose entry costs a request: `version` selects a candidate, and the
/// manifest at that ref supplies the require and autoload rules recorded in the lock.
impl Lockable for VcsProvider {
    async fn lock(&self, name: &PackageName, version: &Version) -> anyhow::Result<PackageDef> {
        if name != &self.name {
            anyhow::bail!(hints::wrong_locked_package(
                self.name.as_str(),
                name.as_str(),
            ));
        }

        self.locked(version).await
    }
}
