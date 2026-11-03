//! PHP version semantics for Phast.
//!
//! PHP / Composer versions are close to semver but diverge in several ways:
//!
//! - Optional `v` or `V` prefix: `v1.2.3` is the same as `1.2.3`
//! - Four-segment versions: `1.2.3.4` (common in older packages)
//! - Stability suffixes in multiple formats:
//!     - `1.0.0-beta1`, `1.0.0-beta.1`, `1.0.0beta1` are all equivalent
//!     - Labels are `dev`, `alpha`, `beta`, `rc`, `stable`, `patch`, plus the
//!       abbreviations `a`, `b` and `pl`, matched without regard to case
//! - `dev-main`, `dev-master`, `dev-{branch}` are branch versions, outside semver entirely
//! - A trailing `-dev` makes a branch of whatever precedes it, so the bleeding-edge
//!   alias `9999999-dev` is a branch named `9999999` rather than a numeric version
//!
//! This crate owns the types those strings parse into and the parsers that read them
//! out of a `phast.toml` or a `composer.json`.

pub mod constraint;
pub mod parser;
pub mod spec;
pub mod stability;
pub mod version;

pub use constraint::Constraint;
pub use spec::{RequireSpec, StabilityFlag, VersionSpec};
pub use stability::Stability;
pub use version::{NumericVersion, Version};
