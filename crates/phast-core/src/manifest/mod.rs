//! Manifest file types: phast.toml and composer.json.
//!
//! Each format has its own serde surface, `PhastManifest` for the TOML and
//! `ComposerManifest` for the JSON, and both convert into one [`Manifest`].
//! That struct records nothing about which file it came from, so no reader
//! downstream can branch on it.
//!
//! `phast.toml` wins when both are present, and `composer.json` is then never opened.
pub mod composer;
pub mod phast;

mod convert;
mod formatter;
mod model;

pub use model::{Autoload, BasicAuth, Config, Repository, Script, Source};
pub use model::{GitRef, PlatformCheck, RepositoryType, VcsType};

use crate::package::PackageName;

use std::collections::{BTreeMap, HashMap};
use std::fs::read_to_string;
use std::path::Path;
use std::str::FromStr;

use phast_version::{RequireSpec, VersionSpec};

use anyhow::{Context, bail};

/// The unified manifest representation used internally by all phast crates.
///
/// This is what both `PhastManifest` and `ComposerManifest` convert into once parsed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Manifest {
    /// Canonical `vendor/package` name from the manifest, if present.
    pub name: Option<String>,
    /// Declared version, if present. Usually absent for application projects
    /// and published packages (the registry derives it), but required for
    /// path-repository and workspace-member packages that must pin a version.
    pub version: Option<String>,
    /// Short human-readable description of the project.
    pub description: Option<String>,
    /// Composer package type, e.g. `library`, `project`, `composer-plugin`.
    pub package_type: Option<String>,
    /// SPDX license identifier, e.g. `MIT` or `Apache-2.0`.
    pub license: Option<String>,
    /// Runtime dependencies declared in `require` / `"require"`. Values are
    /// `RequireSpec` so `self.version` is carried and resolved at the boundary.
    pub require: HashMap<PackageName, RequireSpec>,
    /// Dev-only dependencies declared in `require-dev` / `"require-dev"`.
    pub require_dev: HashMap<PackageName, RequireSpec>,
    /// Packages incompatible with this project from `conflict` / `"conflict"`.
    pub conflict: HashMap<PackageName, RequireSpec>,
    /// Packages this project replaces from `replace` / `"replace"`.
    pub replace: HashMap<PackageName, VersionSpec>,
    /// Virtual packages this project fulfills from `provide` / `"provide"`.
    pub provide: HashMap<PackageName, VersionSpec>,
    /// Resolver and platform configuration from `[config]` / `"config"`.
    pub config: Config,
    /// Autoload rules for production code.
    pub autoload: Autoload,
    /// Autoload rules for dev code (tests, tooling).
    pub autoload_dev: Autoload,
    /// Lifecycle scripts keyed by event name, e.g. `post-install-cmd`.
    pub scripts: BTreeMap<String, Script>,
    /// Additional package repositories declared in the manifest.
    pub repositories: Vec<Repository>,
    /// Explicit per-package source specs declared in `[sources]`.
    ///
    /// Maps a package name to where it is fetched from: a named `[[repositories]]`
    /// endpoint, a single-package git source, a local path, or an inline package.
    /// A package absent from this map is looked for in each configured repository in
    /// turn, in the order the manifest declares them.
    pub sources: HashMap<PackageName, Source>,
    /// Arbitrary extra data passed through from the manifest unchanged.
    pub extra: serde_json::Value,
}

impl Manifest {
    /// Load the project manifest from `dir`, preferring `phast.toml` over
    /// `composer.json`.
    ///
    /// Encodes the canonical precedence (the native format wins; Composer's is
    /// read only as input). Returns an error when neither file is present.
    #[rustfmt::skip]
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let phast_toml = dir.join("phast.toml");
        if phast_toml.is_file() {
            let content = read_to_string(&phast_toml)
                .context("failed to read phast.toml")?;
            return Self::from_phast_toml(&content)
                .context("phast.toml is invalid");
        }

        let composer_json = dir.join("composer.json");
        if composer_json.is_file() {
            let content = read_to_string(&composer_json)
                .context("failed to read composer.json")?;
            return Self::from_composer_json(&content)
                .context("composer.json is invalid");
        }

        bail!(
            "neither phast.toml nor composer.json found in {}",
            dir.display(),
        )
    }

    /// Whether this manifest requires `package`, dev requirements included on request.
    pub fn requires(&self, package: &PackageName, include_dev: bool) -> bool {
        self.require.contains_key(package)
            || (self.require_dev.contains_key(package) && include_dev)
    }

    /// Every package this manifest requires, dev requirements included on request.
    ///
    /// A lookup and a walk are different shapes, which is why this sits beside
    /// [`requires`](Self::requires) rather than being expressed through it. What the pair
    /// exists for is the rule they share: which sections count as required.
    pub fn requirements(&self, include_dev: bool) -> impl Iterator<Item = &PackageName> {
        self.require
            .keys()
            .chain(self.require_dev.keys().filter(move |_| include_dev))
    }

    /// Parse a `phast.toml` string into the domain model.
    pub fn from_phast_toml(raw: &str) -> Result<Self, phast::PhastManifestError> {
        phast::PhastManifest::from_str(raw)?.into_manifest()
    }

    /// Parse a `composer.json` string into the domain model.
    pub fn from_composer_json(raw: &str) -> Result<Self, composer::ComposerManifestError> {
        composer::ComposerManifest::from_str(raw)?.into_manifest()
    }
}
