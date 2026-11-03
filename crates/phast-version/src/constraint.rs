//! Composer version constraints and their translation to version ranges.
//!
//! ## Composer constraint syntax
//!
//! Composer supports a rich constraint language. The forms are:
//!
//! | Syntax         | Meaning                           | Translates to              |
//! |----------------|-----------------------------------|----------------------------|
//! | `^1.2.3`       | Compatible (no breaking changes)  | `>=1.2.3, <2.0.0`          |
//! | `~1.2.3`       | Approximately equal               | `>=1.2.3, <1.3.0`          |
//! | `~1.2`         | Approximately equal (minor)       | `>=1.2.0, <2.0.0`          |
//! | `1.2.*`        | Wildcard patch                    | `>=1.2.0, <1.3.0`          |
//! | `1.*`          | Wildcard minor                    | `>=1.0.0, <2.0.0`          |
//! | `>=1.0 <2.0`   | Range (space = implicit AND)      | intersection               |
//! | `1.0 \|\| 2.0` | OR                                | union                      |
//! | `1.0@beta`     | Version with stability override   | `1.0`, flag read apart     |
//! | `dev-main`     | Branch constraint                 | matches branch version     |
//!
//! Those bounds are written in their simplest form. Each one actually carries a `-dev`
//! sentinel, which is what lets `1.*` match `1.0.0-beta1` while still refusing
//! `2.0.0-alpha1`: a pre-release inside the slot is in, one belonging to the next slot
//! is out.
//!
//! ## Challenge: `^` with non-stable versions
//!
//! `^1.0.0-beta` in Composer means `>=1.0.0-beta, <2.0.0`. The beta suffix
//! affects the lower bound but not the upper, a subtle difference from npm's `^`.
//! It falls out of comparing whole versions rather than segments, since a `Version`
//! carries its own stability and orders by it.
//!
//! ## Challenge: stability flags on ranges
//!
//! `>=1.0@beta` means: at least 1.0, but allow beta versions of packages that
//! would otherwise be filtered by `minimum-stability`. The `@stability` suffix
//! modifies the stability filter, not the version comparison.

use crate::Stability;
use crate::Version;
use crate::version::NumericVersion;

use thiserror::Error;

use std::fmt::{Display, Formatter, Result as FmtResult};
use std::result::Result as StdResult;
use std::str::FromStr;

/// A single parsed version constraint in Composer syntax, as written in
/// phast.toml and composer.json.
///
/// A constraint is either a simple bound, a range (AND of bounds),
/// or a union (OR) of ranges. This maps to PubGrub's `Range<Version>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constraint {
    /// Matches any version: `*`
    Any,
    /// A single exact version: `1.2.3`
    Exact(Version),
    /// A comparison bound: `>=1.0`, `<2.0`, `!=1.5`
    Comparison(Op, Version),
    /// A caret range: `^1.2.3`
    Caret(Version),
    /// A tilde range: `~1.2.3`
    Tilde(Version),
    /// A wildcard: `1.2.*`, `1.*`
    Wildcard { major: u64, minor: Option<u64> },
    /// A branch constraint: `dev-main`
    Branch(String),
    /// AND of multiple constraints (space-separated in Composer): `>=1.0 <2.0`
    And(Vec<Constraint>),
    /// OR of multiple constraints (`||` in Composer)
    Or(Vec<Constraint>),
}

/// Errors produced when parsing a constraint string fails.
#[derive(Debug, Error)]
pub enum ConstraintError {
    /// The input string was empty or contained only whitespace.
    #[error("empty constraint string")]
    Empty,

    /// The input was non-empty but could not be interpreted as a valid constraint.
    #[error("invalid constraint '{0}': {1}")]
    Invalid(String, String),
}

impl Constraint {
    /// Whether `version` satisfies this constraint.
    ///
    /// Mirrors the interval semantics of the resolver's range translation
    /// (`constraint_to_range` in phast-resolver); the two are kept in
    /// agreement by a cross-check test there.
    #[rustfmt::skip]
    pub fn matches(&self, version: &Version) -> bool {
        match self {
            Constraint::Any => true,
            Constraint::Exact(v) => {
                version == v
            },
            Constraint::Branch(name) => {
                matches!(version, Version::Branch(b) if b == name)
            }
            Constraint::Comparison(op, v) => {
                op.satisfied_by(version, v)
            },
            Constraint::Caret(v) => {
                caret_matches(version, v)
            },
            Constraint::Tilde(v) => {
                tilde_matches(version, v)
            },
            Constraint::Wildcard { major, minor } => {
                wildcard_matches(version, *major, *minor)
            },
            Constraint::And(terms) => {
                terms.iter().all(|c| c.matches(version))
            },
            Constraint::Or(terms) => {
                terms.iter().any(|c| c.matches(version))
            },
        }
    }

    /// The lowest version this constraint admits, or `None` when it has no floor.
    ///
    /// Used to compute an effective minimum, e.g. the highest PHP a dependency
    /// set requires. Upper-only bounds (`<`, `<=`, `!=`), `*`, and branches have
    /// no floor.
    #[rustfmt::skip]
    pub fn lower_bound(&self) -> Option<Version> {
        match self {
            Constraint::Exact(v)
          | Constraint::Caret(v)
          | Constraint::Tilde(v)
          | Constraint::Comparison(Op::Gt | Op::Gte, v) => {
                Some(v.clone())
            },
            Constraint::Wildcard { major, minor } => {
                Some(wildcard_bounds(*major, *minor).0)
            },
            Constraint::And(terms) => {
                terms.iter().map(Constraint::lower_bound).max().flatten()
            },
            Constraint::Or(terms) => {
                terms.iter().map(Constraint::lower_bound).min().flatten()
            },
            Constraint::Comparison(..) | Constraint::Any | Constraint::Branch(_) => None,
        }
    }
}

/// Renders the constraint back in Composer syntax: `And` joins parts with
/// spaces, `Or` with ` || `.
#[rustfmt::skip]
impl Display for Constraint {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Constraint::Any => {
                write!(f, "*")
            },
            Constraint::Exact(v) => {
                write!(f, "{}", v)
            },
            Constraint::Comparison(op, v) => {
                write!(f, "{}{}", op, v)
            },
            Constraint::Caret(v) => {
                write!(f, "^{}", v)
            },
            Constraint::Tilde(v) => {
                write!(f, "~{}", v)
            },
            Constraint::Wildcard { major, minor: Some(minor) } => {
                write!(f, "{}.{}.*", major, minor)
            },
            Constraint::Wildcard { major, minor: None } => {
                write!(f, "{}.*", major)
            },
            Constraint::Branch(b) => {
                write!(f, "dev-{}", b)
            },
            Constraint::And(parts) => {
                let s: Vec<String> = parts.iter().map(|c| c.to_string()).collect();
                write!(f, "{}", s.join(" "))
            }
            Constraint::Or(parts) => {
                let s: Vec<String> = parts.iter().map(|c| c.to_string()).collect();
                write!(f, "{}", s.join(" || "))
            }
        }
    }
}

/// Parses Composer constraint syntax; delegates to [`crate::parser::parse_constraint`].
impl FromStr for Constraint {
    type Err = ConstraintError;

    fn from_str(string: &str) -> StdResult<Self, Self::Err> {
        crate::parser::parse_constraint(string)
    }
}

/// Comparison operators used in explicit bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Strictly greater than (`>`).
    Gt,
    /// Greater than or equal to (`>=`).
    Gte,
    /// Strictly less than (`<`).
    Lt,
    /// Less than or equal to (`<=`).
    Lte,
    /// Not equal to (`!=`).
    Ne,
}

#[rustfmt::skip]
impl Op {
    /// Whether `version <op> bound` holds.
    fn satisfied_by(&self, version: &Version, bound: &Version) -> bool {
        match self {
            Op::Gt  => version > bound,
            Op::Gte => version >= bound,
            Op::Lt  => version < bound,
            Op::Lte => version <= bound,
            Op::Ne  => version != bound,
        }
    }
}

/// Renders the operator as written in constraint strings (`>=`, `<`, `!=`, ...).
#[rustfmt::skip]
impl Display for Op {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Op::Gt  => write!(f, ">"),
            Op::Gte => write!(f, ">="),
            Op::Lt  => write!(f, "<"),
            Op::Lte => write!(f, "<="),
            Op::Ne  => write!(f, "!="),
        }
    }
}

/// Half-open bounds spanned by a wildcard constraint.
///
/// Bounds carry `Dev` stability so pre-release versions inside the slot match:
///
/// ```text
/// 1.*   -> [1.0.0.0-Dev, 2.0.0.0-Dev)
/// 1.2.* -> [1.2.0.0-Dev, 1.3.0.0-Dev)
/// ```
pub fn wildcard_bounds(major: u64, minor: Option<u64>) -> (Version, Version) {
    let dev = |segments: [u64; 4]| {
        Version::Numeric(NumericVersion {
            segments,
            stability: Stability::Dev,
            stability_seq: 0,
            precision: 0,
        })
    };

    match minor {
        None => (
            dev([major, 0, 0, 0]),
            dev([major.saturating_add(1), 0, 0, 0]),
        ),
        Some(minor) => (
            dev([major, minor, 0, 0]),
            dev([major, minor.saturating_add(1), 0, 0]),
        ),
    }
}

/// Wildcards match within the half-open slot from [`wildcard_bounds`].
fn wildcard_matches(version: &Version, major: u64, minor: Option<u64>) -> bool {
    let (lower, upper) = wildcard_bounds(major, minor);
    *version >= lower && *version < upper
}

/// `^` locks on the leftmost non-zero segment; see [`NumericVersion::caret_upper`].
fn caret_matches(version: &Version, lower: &Version) -> bool {
    let Version::Numeric(v) = lower else {
        // Branch versions: no range makes sense for a branch caret.
        return version == lower;
    };

    version >= lower && *version < Version::Numeric(v.caret_upper())
}

/// `~` allows patch-level changes when a patch segment is specified,
/// minor-level changes otherwise; see [`NumericVersion::tilde_upper`].
fn tilde_matches(version: &Version, lower: &Version) -> bool {
    let Version::Numeric(v) = lower else {
        return version == lower;
    };

    version >= lower && *version < Version::Numeric(v.tilde_upper())
}
