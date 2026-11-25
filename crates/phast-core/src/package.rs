//! Core package identity and classification types.
//!
//! `PackageName` is the primitive identifier used everywhere. Keeping it
//! as a new type wrapper (rather than a bare `String`) prevents mixing up
//! arguments at call sites.
//!
//! `PackageType` is the curated set of types Phast offers when scaffolding.
//! It is deliberately not the manifest's wire type: `[package] type` stays an
//! `Option<String>` so that any Composer type passes through without data loss.

use std::fmt::{Display, Formatter, Result as FmtResult};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A fully-qualified Composer package name in `vendor/package` format.
///
/// Names are case-insensitive in Composer. Phast normalizes all names to
/// lowercase on construction so comparisons are always byte-equal.
///
/// Platform packages (`php`, `ext-json`, `lib-curl`) use the same type
/// without a vendor prefix.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PackageName(String);

impl PackageName {
    /// Create a new `PackageName`, normalizing the input to lowercase.
    pub fn new(string: impl Into<String>) -> Self {
        Self(string.into().to_ascii_lowercase())
    }

    /// Return the package name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Return `true` if `name` could be a package identifier.
    ///
    /// A slash is taken as sufficient, so `a/` and `a/b/c` pass alongside `vendor/package`.
    /// Platform packages pass on their prefix instead. A name with neither is rejected.
    pub fn is_valid(name: &str) -> bool {
        name.contains('/') || is_platform(name)
    }

    /// Whether `name` is exactly `vendor/package` in Composer's naming rules.
    ///
    /// Each segment starts and ends with a lowercase letter or digit, with `_`, `.` and `-`
    /// allowed between. Stricter than [`is_valid`](Self::is_valid): no third segment, no
    /// empty one, no `.` or `..`, so a name that passes stays inside any directory it is
    /// joined onto.
    pub fn is_well_formed(name: &str) -> bool {
        let Some((vendor, package)) = name.split_once('/') else {
            return false;
        };

        is_name_segment(vendor) && is_name_segment(package)
    }
}

/// Parses a package identifier, normalizing it to lowercase
/// as [`new`](PackageName::new) does.
/// Rejects anything [`is_valid`](PackageName::is_valid) refuses.
impl FromStr for PackageName {
    type Err = String;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        if !Self::is_valid(string) {
            return Err(format!("invalid package name: '{string}'"));
        }

        Ok(Self::new(string))
    }
}

impl Display for PackageName {
    /// Format the package name as its lowercase string representation.
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(&self.0)
    }
}

/// The valid values for the `type` field in `[package]`.
///
/// Only the three types the Phast wizard offers are covered here. The underlying
/// TOML field remains `Option<String>` to pass through arbitrary types like
/// `composer-plugin` or `metapackage` without data loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageType {
    Library,
    Project,
    Plugin,
}

#[rustfmt::skip]
impl PackageType {
    /// All variants in display order.
    pub fn all() -> &'static [PackageType] {
        &[
            PackageType::Library,
            PackageType::Project,
            PackageType::Plugin,
        ]
    }

    /// The canonical string written to `phast.toml`.
    pub fn as_str(self) -> &'static str {
        match self {
            PackageType::Library => "library",
            PackageType::Project => "project",
            PackageType::Plugin  => "phast-plugin",
        }
    }
}

/// Parses the canonical string written to `phast.toml`;
/// the inverse of [`as_str`](PackageType::as_str).
/// Unknown values are rejected.
impl FromStr for PackageType {
    type Err = String;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        Self::all()
            .iter()
            .find(|t| t.as_str() == string)
            .copied()
            .ok_or_else(|| format!("unknown package type: '{string}'"))
    }
}

/// The PHP runtime pseudo-package: `php` and its variants (`php-64bit`,
/// `php-zts`, `php-ipv6`, `php-debug`).
pub fn is_php(name: &str) -> bool {
    !name.contains('/') && (name == "php" || name.starts_with("php-"))
}

/// A PHP extension pseudo-package (`ext-*`, e.g. `ext-json`).
pub fn is_ext(name: &str) -> bool {
    !name.contains('/') && name.starts_with("ext-")
}

/// A system library pseudo-package (`lib-*`, e.g. `lib-curl`).
pub fn is_lib(name: &str) -> bool {
    !name.contains('/') && name.starts_with("lib-")
}

/// A Composer runtime pseudo-package (`composer-*`, e.g. `composer-runtime-api`,
/// `composer-plugin-api`).
pub fn is_composer(name: &str) -> bool {
    !name.contains('/') && name.starts_with("composer-")
}

/// Any platform pseudo-package: the PHP runtime, an extension, a system library,
/// or a Composer runtime API. These only exist as constraints in metadata (they
/// have no registry page), so detecting them avoids unnecessary lookup requests
/// during dependency traversal.
///
/// Real Packagist packages always contain a `/` (vendor/package); platform
/// packages never do. Each helper enforces that, so a `/` is an unambiguous
/// signal of a real package even when the vendor prefix looks platform-like
/// (e.g. `php-debugbar/php-debugbar`).
pub fn is_platform(name: &str) -> bool {
    is_php(name) || is_ext(name) || is_lib(name) || is_composer(name)
}

/// Whether `segment` is one half of a well-formed `vendor/package` name.
fn is_name_segment(segment: &str) -> bool {
    let is_edge = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();

    segment.starts_with(is_edge)
        && segment.ends_with(is_edge)
        && segment.chars().all(|c| is_edge(c) || "_.-".contains(c))
}
