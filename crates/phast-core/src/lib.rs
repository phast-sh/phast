//! Shared domain types for Phast.
//!
//! This crate defines the core data structures that flow between all other crates, and the
//! serialization logic that moves them between formats. `composer.json` is read and never
//! written: `phast.toml` is the format Phast owns and the only one it emits.
//!
//! It makes no network requests, resolves nothing, and runs no subprocesses. Those
//! belong to `phast-registry`, `phast-resolver`, and `phast-runtime` respectively,
//! and keeping them out is what lets every other crate depend on this one.
//!
//! It does touch the filesystem, but only to load and persist the documents modeled
//! here:
//!  - `Manifest::load` reads a project's manifest
//!  - `PhastLock::read`/`write` read and atomically replace the lock
//!  - `git::current_branch` reads `.git/HEAD`.
//!
//! A type that knows its own format is the right place for that; anything beyond reading
//! and writing these files is out of scope.

#[cfg(feature = "schema")]
pub mod schema;

pub mod git;
pub mod lock;
pub mod manifest;
pub mod meta;
pub mod package;
pub mod platform;
pub mod shared;
pub mod toml;

pub use package::{PackageName, PackageType};
