//! `VersionSpec` and `RequireSpec`: deferred values in the dependency maps.
//!
//! Both hold a `self.version` sentinel that only resolves once the declaring
//! package's version is known, so neither can be modeled as a plain value:
//!
//! - [`VersionSpec`] is a `replace` / `provide` value and resolves to a `Version`.
//! - [`RequireSpec`] is a `require` / `require-dev` / `conflict` value and resolves
//!   to a `Constraint` (those sections are ranges, not single versions).

use crate::constraint::{Constraint, ConstraintError};
use crate::parser::{implied_stability, parse_requirement, parse_version};
use crate::stability::Stability;
use crate::version::{Version, VersionError};

use std::fmt::{Display, Formatter, Result as FmtResult};
use std::str::FromStr;

/// A `replace` / `provide` value: the version a package declares it replaces or
/// provides another package at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionSpec {
    /// `self.version`: the declaring package's own version.
    SelfVersion,
    /// `*`: replace-all or provide-any. Like `self.version` it names no version
    /// of its own, so it too resolves to the declaring package's.
    Any,
    /// A concrete version, e.g. `1.2.0`.
    Exact(Version),
}

#[rustfmt::skip]
impl VersionSpec {
    /// The concrete version this spec denotes for a package resolved at
    /// `declaring`. `self.version` and `*` resolve to the declaring version;
    /// an exact version resolves to itself.
    pub fn resolve(&self, declaring: &Version) -> Version {
        match self {
            VersionSpec::SelfVersion | VersionSpec::Any
                                        => declaring.clone(),
            VersionSpec::Exact(version) => version.clone(),
        }
    }
}

/// Composer's sentinel in a `replace`/`provide` map meaning "the same version as
/// the package declaring it" (e.g. `laravel/framework` replacing every
/// `illuminate/*` at `self.version`). It is not a version string, so it is
/// resolved to the declaring package's own version rather than parsed.
const SELF_VERSION: &str = "self.version";

/// Composer's wildcard in a `replace`/`provide` map, meaning "any/all versions"
/// (e.g. `laravel/framework` replacing `spatie/once` at `*`). Like [`SELF_VERSION`]
/// it is not a concrete version, so it resolves to the declaring package's own.
const ANY_VERSION: &str = "*";

#[rustfmt::skip]
impl FromStr for VersionSpec {
    type Err = VersionError;

    /// `self.version` and `*` map to their variants; anything else must parse as a
    /// concrete version. A version *range* (rare in `replace`/`provide`) does not
    /// parse and is therefore an error the caller can skip.
    fn from_str(string: &str) -> Result<Self, Self::Err> {
        match string.trim() {
            SELF_VERSION => Ok(VersionSpec::SelfVersion),
            ANY_VERSION  => Ok(VersionSpec::Any),
                   other => parse_version(other).map(VersionSpec::Exact),
        }
    }
}

#[rustfmt::skip]
impl Display for VersionSpec {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            VersionSpec::SelfVersion    => write!(f, "{SELF_VERSION}"),
            VersionSpec::Any            => write!(f, "{ANY_VERSION}"),
            VersionSpec::Exact(version) => write!(f, "{version}"),
        }
    }
}

/// A `require` / `require-dev` / `conflict` value: a version constraint, or the
/// `self.version` sentinel that resolves to the declaring package's own version.
///
/// Unlike [`VersionSpec`] (replace/provide, which resolves to a single `Version`),
/// this resolves to a [`Constraint`], since require/conflict are ranges. `*` is a
/// valid constraint (`Constraint::Any`), so only `self.version` is a distinct
/// variant here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequireSpec {
    /// `self.version`: exactly the declaring package's own version (`== declaring`).
    SelfVersion,
    /// A concrete constraint (`^7.4`, `>=2.1`, `*`, ...), plus the stability it
    /// asks for, written (`^2.0@dev`) or implied (`dev-master`).
    Constraint {
        /// The version range itself, with any `@flag` already split off.
        constraint: Constraint,
        /// The stability the requirement asks for, or `None` when it names only
        /// stable versions. Honored only for root requirements.
        stability: Option<StabilityFlag>,
    },
    /// An inline alias (`dev-develop as 5.0.9999`): install the `constraint`'s ref
    /// (a branch or exact version), but present it to other packages under the
    /// phantom `alias` so their numeric constraints can match it.
    ///
    /// Honored only for root requirements; in a dependency the `as` clause is
    /// ignored and the entry behaves as the base `constraint` alone (see
    /// [`resolve`](RequireSpec::resolve)). The phantom version is pinned at the
    /// root via [`root_constraint`](RequireSpec::root_constraint), and a synthetic
    /// candidate for it is produced from [`inline_alias`](RequireSpec::inline_alias).
    Aliased {
        /// The ref actually installed, e.g. `Branch("develop")` or an exact version.
        constraint: Constraint,
        /// The phantom version exposed to dependents, e.g. `Numeric(5.0.9999)`.
        alias: Version,
        /// The stability the base ref implies (`dev-develop` → `Dev`), if any.
        stability: Option<StabilityFlag>,
    },
}

/// A parsed inline alias (`dev-develop as 5.0.9999`): the ref to install and the
/// phantom version to expose for it. Carried from a root `require` to the provider
/// so a synthetic candidate at `alias` can be offered for the `reference` branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineAlias {
    /// The base ref the alias points at, e.g. `Branch("develop")`.
    pub reference: Constraint,
    /// The phantom version the ref is offered under, e.g. `Numeric(5.0.9999)`.
    pub alias: Version,
}

/// The stability a root requirement asks for, and how it asked.
///
/// The two differ in what they may do to the project's `minimum-stability`:
/// a written flag replaces it for the package, while an implied one may only
/// loosen it, so `1.0.0-beta2` under a `dev` project does not drop that
/// package's dev releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StabilityFlag {
    /// Written as an `@flag` (`^2.0@dev`). Usually loosens the floor, though
    /// `@stable` under a `dev` project tightens it.
    Explicit(Stability),
    /// Implied by naming an unstable version with no `@flag` (`dev-master`,
    /// `1.0.0-beta2`). Never rendered, so `dev-master` stays `dev-master`.
    Implied(Stability),
}

#[rustfmt::skip]
impl StabilityFlag {
    /// The floor this flag sets for its package, given the project's own: a
    /// written flag sets it outright, an implied one only when it is lower.
    pub fn floor(self, project: Stability) -> Option<Stability> {
        match self {
            StabilityFlag::Explicit(s) => Some(s),
            StabilityFlag::Implied(s)  => (s < project).then_some(s),
        }
    }
}

impl RequireSpec {
    /// The concrete constraint this spec denotes for a package resolved at
    /// `declaring`: `self.version` becomes `== declaring`; a constraint is itself.
    ///
    /// An inline alias resolves to its *base* constraint, not its phantom version:
    /// this is the transitive-safe reading (a dependency's `dev-x as 1.0` means
    /// `dev-x`), while the root pins the phantom version through
    /// [`root_constraint`](Self::root_constraint).
    pub fn resolve(&self, declaring: &Version) -> Constraint {
        match self {
            RequireSpec::SelfVersion => Constraint::Exact(declaring.clone()),
            RequireSpec::Constraint { constraint, .. } => constraint.clone(),
            RequireSpec::Aliased { constraint, .. } => constraint.clone(),
        }
    }

    /// The constraint a **root** requirement contributes to resolution. Identical to
    /// [`resolve`](Self::resolve) except an inline alias pins its phantom version
    /// (`dev-develop as 5.0.9999` → `== 5.0.9999`), so the root and any dependents
    /// unify on the one synthetic candidate that the phantom version offers.
    pub fn root_constraint(&self, declaring: &Version) -> Constraint {
        match self {
            RequireSpec::Aliased { alias, .. } => Constraint::Exact(alias.clone()),
            _ => self.resolve(declaring),
        }
    }

    /// The inline alias this spec declares, if any — the ref to install paired with
    /// the phantom version to offer for it. Collected from root requirements so the
    /// provider can synthesize the matching candidate.
    pub fn inline_alias(&self) -> Option<InlineAlias> {
        match self {
            RequireSpec::Aliased {
                constraint, alias, ..
            } => Some(InlineAlias {
                reference: constraint.clone(),
                alias: alias.clone(),
            }),
            _ => None,
        }
    }

    /// The stability the requirement asks for, written or implied.
    /// `self.version` never asks for one.
    pub fn stability(&self) -> Option<StabilityFlag> {
        match self {
            RequireSpec::Constraint { stability, .. } => *stability,
            RequireSpec::Aliased { stability, .. } => *stability,
            RequireSpec::SelfVersion => None,
        }
    }
}

impl FromStr for RequireSpec {
    type Err = ConstraintError;

    /// `self.version` maps to its variant; a `<ref> as <version>` inline alias maps
    /// to [`Aliased`](RequireSpec::Aliased); anything else parses as a constraint,
    /// capturing any trailing `@stability` flag (`*` maps to `Constraint::Any`).
    fn from_str(string: &str) -> Result<Self, Self::Err> {
        let trimmed = string.trim();

        if trimmed == SELF_VERSION {
            return Ok(RequireSpec::SelfVersion);
        }

        // Composer inline alias: `<ref> as <version>`. The left is the ref actually
        // installed; the right is the phantom version it is offered under. Only a
        // branch or exact version may be aliased (matching Composer) — a range on
        // the left makes no sense as a single installable ref and is rejected.
        if let Some((left, right)) = trimmed.split_once(" as ") {
            let (constraint, flag) = parse_requirement(left)?;
            if !matches!(constraint, Constraint::Branch(_) | Constraint::Exact(_)) {
                return Err(ConstraintError::Invalid(
                    trimmed.to_owned(),
                    "inline alias may only alias a branch or an exact version".to_owned(),
                ));
            }
            let alias = parse_version(right.trim())
                .map_err(|error| ConstraintError::Invalid(trimmed.to_owned(), error.to_string()))?;
            let stability = stability_flag(left, flag);
            return Ok(RequireSpec::Aliased {
                constraint,
                alias,
                stability,
            });
        }

        let (constraint, flag) = parse_requirement(trimmed)?;
        Ok(RequireSpec::Constraint {
            constraint,
            stability: stability_flag(trimmed, flag),
        })
    }
}

#[rustfmt::skip]
impl Display for RequireSpec {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            RequireSpec::SelfVersion
                => write!(f, "{SELF_VERSION}"),
            RequireSpec::Constraint { constraint, stability: Some(StabilityFlag::Explicit(s)) }
                => write!(f, "{constraint}@{}", s.as_str()),
            RequireSpec::Constraint { constraint, .. }
                => write!(f, "{constraint}"),
            RequireSpec::Aliased { constraint, alias, stability: Some(StabilityFlag::Explicit(s)) }
                => write!(f, "{constraint}@{} as {alias}", s.as_str()),
            RequireSpec::Aliased { constraint, alias, .. }
                => write!(f, "{constraint} as {alias}"),
        }
    }
}

/// The stability flag a requirement string carries: an explicit `@flag` if present,
/// otherwise the one its text implies by naming an unstable version. Shared by the
/// plain and inline-alias parse paths.
fn stability_flag(text: &str, explicit: Option<Stability>) -> Option<StabilityFlag> {
    match explicit {
        Some(s) => Some(StabilityFlag::Explicit(s)),
        None => implied_stability(text).map(StabilityFlag::Implied),
    }
}
