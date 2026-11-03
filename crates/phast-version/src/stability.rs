//! Stability levels as defined by Composer.
//!
//! Ordering matters: a package at a lower stability level is considered
//! "less stable" and will be filtered out when `minimum-stability` is set higher.
//!
//! The discriminants ascend with stability so the derived `Ord` already reads the
//! right way round, and a floor check is a plain `>=`. Composer numbers them the
//! other way, `stable` at 0 rising to `dev` at 20, so matching its constants would
//! invert every comparison here.

use std::fmt::{Display, Formatter, Result as FmtResult};

/// Stability level of a package version.
///
/// Variants are ordered from least stable to most stable.
/// `Dev < Alpha < Beta < RC < Stable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Stability {
    /// Development version (`dev-main`, `9999999-dev`). Least stable.
    Dev = 0,
    /// Alpha release (`1.0.0-alpha1`).
    Alpha = 1,
    /// Beta release (`1.0.0-beta1`).
    Beta = 2,
    /// Release candidate (`1.0.0-RC1`).
    RC = 3,
    /// Stable release. Default when no suffix is present.
    #[default]
    Stable = 4,
}

impl Stability {
    /// Parse a stability label from the suffix of a version string.
    ///
    /// Handles the various formats Composer accepts:
    /// `dev`, `alpha`, `a`, `beta`, `b`, `RC`, `rc`, `patch`, `pl`, `stable`,
    /// in any casing. Returns `None` if the string is not a recognized label.
    ///
    /// Not the inverse of [`as_str`](Self::as_str), which emits one spelling per
    /// level: `pl` and `patch` both arrive as `Stable` and leave as `stable`.
    pub fn from_label(label: &str) -> Option<Self> {
        match label.to_ascii_lowercase().as_str() {
            "dev" => Some(Self::Dev),
            "alpha" | "a" => Some(Self::Alpha),
            "beta" | "b" => Some(Self::Beta),
            "rc" => Some(Self::RC),
            "stable" | "pl" | "patch" => Some(Self::Stable),
            _ => None,
        }
    }

    /// The string label Composer uses in output and lock files.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Alpha => "alpha",
            Self::Beta => "beta",
            Self::RC => "RC",
            Self::Stable => "stable",
        }
    }
}

impl Display for Stability {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(self.as_str())
    }
}
