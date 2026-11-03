//! Source configuration: repositories, per-package sources, and their tag enums.

use std::collections::HashMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A configured repository entry.
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Repository {
    /// A version-control repository.
    Vcs {
        url: String,
        /// Logical name used to reference this repository in `[sources]`.
        name: Option<String>,
    },
    /// A Composer-type repository (Packagist-compatible API).
    Composer {
        url: String,
        /// Logical name used to reference this repository in `[sources]`.
        name: Option<String>,
        /// When `true`, all packages not listed in `[sources]` are fetched from here.
        default: bool,
    },
    /// Packagist itself, redirected or switched off.
    Packagist {
        /// Replaces the default registry URL. `None` leaves no default registry at all,
        /// which is how Packagist is disabled.
        url: Option<String>,
        /// Logical name used to reference this repository in `[sources]`.
        name: Option<String>,
        /// When `true`, all packages not listed in `[sources]` are fetched from here.
        default: bool,
    },
    /// A local path repository (symlinked from a directory on disk).
    Path {
        /// Filesystem path to the local package directory.
        path: String,
        /// Composer-compatible options (e.g. `symlink`, `versions`).
        options: HashMap<String, serde_json::Value>,
    },
    /// An inline package definition.
    Package {
        package: serde_json::Value,
    },
}

/// A per-package source association from `[sources]`.
///
/// Each entry pins exactly one package to where it is fetched from. This is both
/// the dependency-confusion control (a non-registry package must be named here,
/// explicitly) and the way a lone git / path / inline package is attached without a
/// full `[[repositories]]` endpoint.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// The package comes from the named `[[repositories]]` endpoint.
    Repository {
        /// The `name` of the `[[repositories]]` entry to fetch this package from.
        repository: String,
    },
    /// A single-package VCS source. Phast fetches the repo and reads its own
    /// manifest for the package's metadata.
    Git {
        /// Clone URL of the repository.
        url: String,
        /// The ref to check out: a branch, tag, commit, or the default branch.
        reference: GitRef,
    },
    /// A single local package on disk, materialized by symlink.
    Path {
        /// Filesystem path to the package directory, relative to the manifest.
        path: String,
        /// Supplies the package version when its own manifest does not declare one.
        version: Option<String>,
    },
    /// A fully self-describing inline package: it declares its own `version` (and
    /// optionally `require` / `autoload` / `dist`) in the spec itself, so it is
    /// never fetched for metadata. Stored as the Composer package shape.
    Package {
        /// The inline package definition.
        package: serde_json::Value,
    },
}

/// The git ref a [`Source::Git`] source resolves to.
#[derive(Debug, Clone, PartialEq)]
pub enum GitRef {
    /// A named branch.
    Branch(String),
    /// A tag.
    Tag(String),
    /// An exact commit reference.
    Reference(String),
    /// No ref given: use the repository's default branch.
    Default,
}

/// The VCS backend a package's source lives in, as reported by registry metadata
/// (`source.type`) and recorded in the lock.
///
/// This describes a resolved package's origin; it is not a capability claim and not
/// repository configuration.
///
/// The `type` a user writes on a `[[repositories]]` entry is [`RepositoryType`],
/// and which host drives a URL is decided by `host_for`, never by a declared type.
/// Git is the only backend Phast installs from; anything else a registry reports
/// is preserved verbatim as [`VcsType::Other`] so metadata round-trips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VcsType {
    /// Git repository.
    Git,
    /// Any other backend, `svn` or `hg` among them. Not installable.
    #[serde(untagged)]
    Other(String),
}

/// The `type` of a `[[repositories]]` entry: what a user writes in a manifest.
///
/// `Vcs` and `Git` are two spellings of the same thing and behave identically, since
/// the host driver is chosen from the URL (`host_for`), not from this type. Both are
/// accepted because both appear in real manifests.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepositoryType {
    /// Generic VCS repository (the backend is detected from the URL).
    Vcs,
    /// Explicit Git repository.
    Git,
    /// A Packagist-style registry (v2/v1 API).
    Composer,
    /// Local path repository.
    Path,
    /// Inline package definition.
    Package,
    /// Packagist (the default registry); can be disabled or redirected.
    Packagist,
}

impl FromStr for RepositoryType {
    type Err = String;

    #[rustfmt::skip]
    fn from_str(string: &str) -> Result<Self, Self::Err> {
        match string {
            "vcs"       => Ok(Self::Vcs),
            "git"       => Ok(Self::Git),
            "composer"  => Ok(Self::Composer),
            "path"      => Ok(Self::Path),
            "package"   => Ok(Self::Package),
            "packagist" => Ok(Self::Packagist),
            other       => Err(format!("unknown repository type: '{other}'")),
        }
    }
}
