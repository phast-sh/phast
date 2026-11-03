//! The core `Version` type representing a resolved PHP package version.
//!
//! ## Design notes
//!
//! `Version` must implement `Ord` because PubGrub's range operations require
//! a total order over versions. The ordering is:
//!
//!   1. Compare numeric segments lexicographically (major, minor, patch, build).
//!      A missing segment is treated as 0: `1.2` == `1.2.0` == `1.2.0.0`.
//!   2. If numeric segments are equal, compare by stability:
//!      `Dev < Alpha < Beta < RC < Stable`.
//!   3. If stability is also equal, compare by the stability sequence number:
//!      `beta1 < beta2`.
//!
//! Branch versions (`dev-main`, `dev-master`) are a separate variant. A total order
//! still has to place them, so every branch sorts above every numeric version and
//! branches sort alphabetically among themselves. That placement exists to satisfy
//! `Ord`, not to mean anything: a branch is selected by a constraint naming it.

use crate::Stability;

use thiserror::Error;

use std::cmp::Ordering;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::hash::{Hash, Hasher};
use std::result::Result as StdResult;
use std::str::FromStr;

/// Sentinel a `NumericVersion` segment holds for the `x`/`X` placeholder in a
/// branch-alias (`1.2.x-dev`): it must sort above any real release in the same
/// series. A high sentinel rather than a literal `9999999`, which a date-based
/// (CalVer) release like `20031129` would exceed.
pub(crate) const BRANCH_ALIAS_SENTINEL: u64 = u64::MAX >> 1;

/// A parsed PHP package version.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Version {
    /// A numeric version with optional stability suffix.
    /// Examples: `1.2.3`, `2.0.0-beta1`, `1.0.0.0`
    Numeric(NumericVersion),
    /// A branch version, holding the branch name on its own: `dev-main` parses to
    /// `Branch("main")`, and `Display` puts the `dev-` back on the way out.
    Branch(String),
}

/// Errors produced when parsing a version string fails.
#[derive(Debug, Error)]
pub enum VersionError {
    /// The input string was empty or contained only whitespace.
    #[error("empty version string")]
    Empty,

    /// A numeric segment could not be parsed as a `u64`.
    #[error("invalid version segment '{0}': expected a number")]
    InvalidSegment(String),

    /// More than four dot-separated numeric segments were present.
    #[error("too many version segments in '{0}': maximum is 4")]
    TooManySegments(String),

    /// The string did not match any recognized version format.
    #[error("unrecognized version format: '{0}'")]
    Unrecognized(String),
}

impl PartialOrd for Version {
    /// Total order: all `Version` variants are always comparable, so this always returns `Some`.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    /// Numeric versions sort before branch versions; branches sort alphabetically among themselves.
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Version::Numeric(a), Version::Numeric(b)) => a.cmp(b),
            // Numeric versions sort before branch versions.
            (Version::Numeric(_), Version::Branch(_)) => Ordering::Less,
            (Version::Branch(_), Version::Numeric(_)) => Ordering::Greater,
            // Branch versions sort alphabetically among themselves.
            (Version::Branch(a), Version::Branch(b)) => a.cmp(b),
        }
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Version::Numeric(v) => write!(f, "{}", v),
            Version::Branch(b) => write!(f, "dev-{}", b),
        }
    }
}

impl FromStr for Version {
    type Err = VersionError;

    fn from_str(string: &str) -> StdResult<Self, Self::Err> {
        crate::parser::parse_version(string)
    }
}

/// The numeric form of a PHP version.
///
/// `precision` records how many segments were present in the original input
/// string (e.g. 2 for `1.2`, 3 for `1.2.0`). It is metadata about the input,
/// not part of the version identity: two versions that differ only in precision
/// are equal. Synthetic versions produced internally carry `precision: 0`.
#[derive(Debug, Clone)]
pub struct NumericVersion {
    /// Up to four numeric segments. Missing trailing segments are 0.
    /// Stored as exactly four elements for simpler comparison.
    pub segments: [u64; 4],
    /// Stability level parsed from the version suffix.
    pub stability: Stability,
    /// Sequence number within the stability level, e.g. 2 in `beta2`.
    /// Zero when absent.
    pub stability_seq: u32,
    /// Number of segments present in the original input string.
    /// Used by constraint logic (e.g. tilde ranges) to distinguish `~1.2`
    /// (2 segments) from `~1.2.0` (3 segments). Not part of version identity.
    pub precision: u8,
}

impl NumericVersion {
    /// Return a new version with the major segment incremented and everything else
    /// zeroed, at `Dev` stability.
    ///
    /// The `Dev` is what makes this usable as an exclusive upper bound: a pre-release
    /// of the next major sorts at or above it, so the range shuts it out too.
    pub fn next_major(&self) -> Self {
        Self {
            segments: [self.segments[0].saturating_add(1), 0, 0, 0],
            stability: Stability::Dev,
            stability_seq: 0,
            precision: 0,
        }
    }

    /// Return a new version with the minor segment incremented and patch/build zeroed,
    /// at `Dev` stability.
    pub fn next_minor(&self) -> Self {
        Self {
            segments: [self.segments[0], self.segments[1].saturating_add(1), 0, 0],
            stability: Stability::Dev,
            stability_seq: 0,
            precision: 0,
        }
    }

    /// Return a new version with the patch segment incremented and build zeroed,
    /// at `Dev` stability.
    pub fn next_patch(&self) -> Self {
        Self {
            segments: [
                self.segments[0],
                self.segments[1],
                self.segments[2].saturating_add(1),
                0,
            ],
            stability: Stability::Dev,
            stability_seq: 0,
            precision: 0,
        }
    }

    /// Exclusive upper bound of a caret (`^`) range: locks on the leftmost
    /// non-zero segment.
    ///
    /// When all segments are zero, `precision` distinguishes the two forms:
    ///
    /// ```text
    /// ^1.x.y  -> 2.0.0.0-Dev
    /// ^0.x.y  -> 0.(x+1).0.0-Dev  where x > 0
    /// ^0.0.y  -> 0.0.(y+1).0-Dev  patch explicitly written
    /// ^0.0    -> 0.1.0.0-Dev      patch omitted
    /// ```
    pub fn caret_upper(&self) -> Self {
        let [major, minor, _patch, _build] = self.segments;
        if major > 0 {
            self.next_major()
        } else if minor == 0 && self.precision >= 3 {
            // ^0.0.y: patch explicitly written, lock to next patch
            self.next_patch()
        } else {
            // ^0.x.y or ^0.0: lock to next minor
            self.next_minor()
        }
    }

    /// Exclusive upper bound of a tilde (`~`) range: next minor when a patch
    /// segment was written, next major otherwise.
    ///
    /// ```text
    /// ~1.2.3  -> 1.3.0.0-Dev
    /// ~1.2    -> 2.0.0.0-Dev
    /// ~1      -> 2.0.0.0-Dev
    /// ```
    pub fn tilde_upper(&self) -> Self {
        // `precision` records how many segments were written in the original
        // input: `~1.2.0` (precision 3) locks to the next minor like `~1.2.3`.
        if self.precision >= 3 {
            self.next_minor()
        } else {
            self.next_major()
        }
    }
}

impl PartialEq for NumericVersion {
    fn eq(&self, other: &Self) -> bool {
        self.segments == other.segments
            && self.stability == other.stability
            && self.stability_seq == other.stability_seq
    }
}

impl Eq for NumericVersion {}

impl Hash for NumericVersion {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.segments.hash(state);
        self.stability.hash(state);
        self.stability_seq.hash(state);
    }
}

impl PartialOrd for NumericVersion {
    /// Total order: numeric versions are always comparable, so this always returns `Some`.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NumericVersion {
    /// Compare by segments first, then stability level, then stability sequence number.
    fn cmp(&self, other: &Self) -> Ordering {
        self.segments
            .cmp(&other.segments)
            .then(self.stability.cmp(&other.stability))
            .then(self.stability_seq.cmp(&other.stability_seq))
    }
}

impl Display for NumericVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        // A branch-alias placeholder segment (`1.2.x-dev`) renders as `x`,
        // not the sentinel number it sorts as.
        let seg = |value: u64| {
            if value == BRANCH_ALIAS_SENTINEL {
                "x".to_owned()
            } else {
                value.to_string()
            }
        };
        let [a, b, c, d] = self.segments.map(seg);

        // Omit trailing zero segments for clean output: `1.2.0.0` -> `1.2`
        if self.segments[3] != 0 {
            write!(f, "{a}.{b}.{c}.{d}")?;
        } else if self.segments[2] != 0 {
            write!(f, "{a}.{b}.{c}")?;
        } else {
            write!(f, "{a}.{b}")?;
        }

        if self.stability != Stability::Stable {
            if self.stability_seq > 0 {
                write!(f, "-{}{}", self.stability, self.stability_seq)?;
            } else {
                write!(f, "-{}", self.stability)?;
            }
        }

        Ok(())
    }
}
