//! Parsers for PHP version strings and constraint expressions.
//!
//! The two are not independent. Every bound inside a constraint is itself a version,
//! so constraint parsing is layered on version parsing rather than sitting beside it,
//! and a change to what counts as a version reaches both.

mod constraint;
mod version;

pub use constraint::{implied_stability, parse_constraint, parse_requirement};
pub use version::{parse_branch_alias, parse_declared_version, parse_version};
