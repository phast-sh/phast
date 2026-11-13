//! [`InlineProvider`]: a package declared inline in `[sources]`.

use crate::hints;
use crate::p2::convert::convert_extra;
use crate::provider::Lockable;
use crate::vcs::{Host, VcsHost};

use phast_core::lock::phast::{DistSection, ExtraSection, PackageDef, SourceRef};
use phast_core::manifest::VcsType;
use phast_core::package::PackageName;
use phast_resolver::provider::{PackageMetadata, Provider};
use phast_version::parser::parse_declared_version;
use phast_version::{Constraint, RequireSpec, Version};

use anyhow::Context;
use serde_json::Value as JsonValue;

/// A package resolved entirely from an inline `[sources]` definition.
///
/// The spec is self-describing (name, version, git source, optional `require`),
/// so resolution needs no network: `versions` and `metadata` answer from the spec.
/// Only installation fetches the archive, whose dist is synthesized from the git
/// source via the same host machinery as [`VcsProvider`](super::VcsProvider).
#[derive(Clone)]
pub struct InlineProvider {
    /// Declared package name.
    name: PackageName,
    /// Declared version.
    version: Version,
    /// Runtime dependencies declared inline, resolved to constraints.
    require: Vec<(PackageName, Constraint)>,
    /// Archive to fetch at install time, synthesized from the git source.
    dist: DistSection,
    /// Git source recorded in the lock.
    source: SourceRef,
    /// Composer package type, if declared.
    package_type: Option<String>,
    /// Free-form `extra` block declared inline, carried into the lock.
    extra: Option<ExtraSection>,
}

#[rustfmt::skip]
impl InlineProvider {
    /// The package name this provider answers for.
    pub fn name(&self) -> &PackageName {
        &self.name
    }

    /// Build a provider from an inline package definition in the Composer package shape.
    ///
    /// `name`, `version`, and the source's `url` and `reference` are required, and each
    /// missing one names the package in its error. `host` is the git source host, which
    /// the router resolves and rejects before reaching here, so the archive dist can
    /// always be synthesized from it.
    pub fn build(package: &JsonValue, host: Host) -> anyhow::Result<Self> {
        let name = package
            .get("name")
            .and_then(JsonValue::as_str)
            .context("inline package declares no name")?;
        let package_name = PackageName::new(name);
        let package_type = package
            .get("type")
            .and_then(JsonValue::as_str)
            .map(str::to_owned);

        let version = package
            .get("version")
            .and_then(JsonValue::as_str)
            .with_context(|| format!("inline package '{}' declares no version", package_name.as_str()))?;
        let version = parse_declared_version(version)
            .with_context(|| format!("invalid version for inline package '{}'", package_name.as_str()))?;
        let require = inline_require(package.get("require"), &version)?;

        let src = package
            .get("source");
        let url = src
            .and_then(|s| s.get("url"))
            .and_then(JsonValue::as_str)
            .with_context(|| format!("inline package '{}' declares no source url", package_name.as_str()))?;
        let reference = src
            .and_then(|s| s.get("reference"))
            .and_then(JsonValue::as_str)
            .with_context(|| format!("inline package '{}' declares no source reference", package_name.as_str()))?;

        let source = SourceRef {
            vcs_type: VcsType::Git,
            url: url.to_owned(),
            reference: reference.to_owned(),
        };

        Ok(Self {
            name: package_name,
            version,
            require,
            dist: host.dist(reference),
            source,
            package_type,
            extra: package.get("extra").cloned().and_then(convert_extra),
        })
    }

    /// The locked entry: the declared version, synthesized dist, and git source.
    fn locked(&self) -> PackageDef {
        let require = self
            .require
            .iter()
            .map(|(name, constraint)| {
                (name.as_str().to_owned(), constraint.to_string())
            })
            .collect();

        PackageDef {
            name: self.name.as_str().to_owned(),
            version: self.version.to_string(),
            require,
            dist: Some(self.dist.clone()),
            source: Some(self.source.clone()),
            package_type: self.package_type.clone(),
            extra: self.extra.clone(),
            ..Default::default()
        }
    }
}

/// Answers from the spec, never from the network.
///
/// An inline package is registered in the routing table under its own name, so it never
/// joins the fallback chain and is only ever asked about itself. That invariant is what
/// lets `metadata` echo back the name it was handed instead of checking it.
#[rustfmt::skip]
impl Provider for InlineProvider {
    async fn versions(&self, name: &PackageName) -> anyhow::Result<Vec<Version>> {
        if name != &self.name { return Ok(vec![]); }

        Ok(
            vec![self.version.clone()],
        )
    }

    async fn metadata(
        &self,
        name: &PackageName,
        version: &Version,
    ) -> anyhow::Result<PackageMetadata> {
        Ok(PackageMetadata {
            name: name.clone(),
            version: version.clone(),
            require: self.require.clone(),
            require_dev: vec![],
            replace: vec![],
            provide: vec![],
        })
    }
}

/// The spec is the package, so the entry is built from memory: no I/O;
/// `version` is ignored because an inline definition declares exactly one.
impl Lockable for InlineProvider {
    async fn lock(&self, name: &PackageName, _version: &Version) -> anyhow::Result<PackageDef> {
        if name != &self.name {
            anyhow::bail!(hints::wrong_locked_package(
                self.name.as_str(),
                name.as_str(),
            ));
        }

        Ok(self.locked())
    }
}

/// Parse an inline package's `require` map into constraints resolved against
/// `declaring`, so a `self.version` requirement binds to the declared version.
fn inline_require(
    value: Option<&JsonValue>,
    declaring: &Version,
) -> anyhow::Result<Vec<(PackageName, Constraint)>> {
    let Some(map) = value.and_then(JsonValue::as_object) else {
        return Ok(vec![]);
    };

    map.iter()
        .map(|(name, constraint)| {
            let _c = constraint
                .as_str()
                .with_context(|| format!("require constraint for '{name}' must be a string"))?;
            let spec = _c
                .parse::<RequireSpec>()
                .with_context(|| format!("invalid constraint '{_c}' for '{name}'"))?;
            Ok((PackageName::new(name), spec.resolve(declaring)))
        })
        .collect()
}
