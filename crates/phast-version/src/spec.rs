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
    pub fn resolve(&self, declaring: &Version) -> Constraint {
        match self {
            RequireSpec::SelfVersion => Constraint::Exact(declaring.clone()),
            RequireSpec::Constraint { constraint, .. } => constraint.clone(),
        }
    }

    /// The stability the requirement asks for, written or implied.
    /// `self.version` never asks for one.
    pub fn stability(&self) -> Option<StabilityFlag> {
        match self {
            RequireSpec::Constraint { stability, .. } => *stability,
            RequireSpec::SelfVersion => None,
        }
    }
}

impl FromStr for RequireSpec {
    type Err = ConstraintError;

    /// `self.version` maps to its variant; anything else parses as a constraint,
    /// capturing any trailing `@stability` flag (`*` maps to `Constraint::Any`).
    fn from_str(string: &str) -> Result<Self, Self::Err> {
        match string.trim() {
            SELF_VERSION => Ok(RequireSpec::SelfVersion),
            other => {
                let (constraint, flag) = parse_requirement(other)?;
                let stability = match flag {
                    Some(s) => Some(StabilityFlag::Explicit(s)),
                    None => implied_stability(other).map(StabilityFlag::Implied),
                };
                Ok(RequireSpec::Constraint {
                    constraint,
                    stability,
                })
            }
        }
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
        }
    }
}
