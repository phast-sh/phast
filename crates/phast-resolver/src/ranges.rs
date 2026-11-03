//! Adapter: translate a `Constraint` into a `pubgrub::Ranges<Version>`.
//!
//! This is the bridge between phast-version's domain types and PubGrub's
//! interval algebra. It lives here rather than in phast-version so that
//! phast-version stays free of a pubgrub dependency.

use std::ops::Bound;

use phast_version::constraint::{Op, wildcard_bounds};
use phast_version::{Constraint, NumericVersion, Stability, Version};

use pubgrub::Ranges;

/// Convert a `Constraint` to the equivalent `Ranges<Version>` for PubGrub.
pub fn constraint_to_range(c: &Constraint) -> Ranges<Version> {
    match c {
        Constraint::Any => Ranges::full(),
        Constraint::Exact(version) => Ranges::singleton(version.clone()),
        Constraint::Branch(name) => Ranges::singleton(Version::Branch(name.clone())),
        Constraint::Comparison(op, version) => comparison_range(op, version),
        Constraint::Caret(version) => caret_range(version),
        Constraint::Tilde(version) => tilde_range(version),
        Constraint::Wildcard { major, minor } => wildcard_range(*major, *minor),
        Constraint::And(terms) => terms
            .iter()
            .map(constraint_to_range)
            .fold(Ranges::full(), |c, r| c.intersection(&r)),
        Constraint::Or(terms) => terms
            .iter()
            .map(constraint_to_range)
            .fold(Ranges::empty(), |c, r| c.union(&r)),
    }
}

/// Return a version that satisfies `range` by reading its lower bound.
///
/// The result stands in for a virtual package with no known providers yet, and is
/// filtered out of the final resolution, so any version inside the range serves
/// equally well. The lower bound is simply the cheapest to reach without searching.
///
/// An inclusive bound comes back as it stands. An exclusive numeric one has its
/// build segment, the fourth, raised by one to land strictly inside the range.
/// An exclusive branch bound comes back unchanged, having no segment to step, and
/// a range with no lower bound at all yields `0.0.0.0`.
pub fn range_to_version(range: &Ranges<Version>) -> Version {
    let fallback = Version::Numeric(NumericVersion {
        segments: [0, 0, 0, 0],
        stability: Stability::Stable,
        stability_seq: 0,
        precision: 0,
    });

    let Some((lower, _)) = range.bounding_range() else {
        return fallback;
    };

    match lower {
        Bound::Included(v) => v.clone(),
        Bound::Excluded(Version::Numeric(v)) => {
            let mut segments = v.segments;
            segments[3] = segments[3].saturating_add(1);

            Version::Numeric(NumericVersion {
                segments,
                stability: v.stability,
                stability_seq: v.stability_seq,
                precision: 0,
            })
        }
        Bound::Excluded(v) => v.clone(),
        Bound::Unbounded => fallback,
    }
}

/// Convert a single comparison operator and version into a `Ranges<Version>`.
#[rustfmt::skip]
fn comparison_range(op: &Op, v: &Version) -> Ranges<Version> {
    match op {
        Op::Gte => Ranges::higher_than(v.clone()),
        Op::Gt  => Ranges::strictly_higher_than(v.clone()),
        Op::Lte => Ranges::lower_than(v.clone()),
        Op::Lt  => Ranges::strictly_lower_than(v.clone()),
        Op::Ne  => Ranges::strictly_lower_than(v.clone())
            .union(&Ranges::strictly_higher_than(v.clone()))
    }
}

/// `^` spans `[version, caret_upper)`; bound rules live on [`NumericVersion::caret_upper`].
fn caret_range(version: &Version) -> Ranges<Version> {
    let v = match version {
        Version::Numeric(v) => v,
        // Branch versions: treat as singleton (no range makes sense for a branch caret).
        Version::Branch(_) => return Ranges::singleton(version.clone()),
    };

    Ranges::between(version.clone(), Version::Numeric(v.caret_upper()))
}

/// `~` spans `[version, tilde_upper)`; bound rules live on [`NumericVersion::tilde_upper`].
fn tilde_range(version: &Version) -> Ranges<Version> {
    let v = match version {
        Version::Numeric(v) => v,
        Version::Branch(_) => return Ranges::singleton(version.clone()),
    };

    Ranges::between(version.clone(), Version::Numeric(v.tilde_upper()))
}

/// Wildcards span the half-open slot from [`wildcard_bounds`].
fn wildcard_range(major: u64, minor: Option<u64>) -> Ranges<Version> {
    let (lower, upper) = wildcard_bounds(major, minor);
    Ranges::between(lower, upper)
}
