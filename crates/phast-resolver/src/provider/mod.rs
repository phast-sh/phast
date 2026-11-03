//! The `Provider` trait: the interface between the resolver and the outside world.
//!
//! The resolver calls into a `Provider` to answer two questions:
//! 1. What versions of package X exist, in preference order?
//! 2. What are the dependencies of package X at version V?
//!
//! `phast-registry` implements it against whatever a package's source turns out
//! to be, a Packagist-compatible API among them. Test providers implement it
//! against hardcoded in-memory data.
//!
//! ## Async design
//!
//! PubGrub's `DependencyProvider` trait is synchronous. phast wraps it
//! with an async `Provider` trait that the registry implements, and bridges
//! to PubGrub via a blocking executor on a separate thread pool.
//!
//! This is the same approach uv uses: the resolver runs synchronously on
//! a dedicated thread, while I/O runs on tokio. The bridge sends fetch
//! requests through a channel and blocks until the response arrives.

pub mod memory;
pub mod pubgrub;

use std::future::Future;

use phast_core::package::PackageName;
use phast_version::{Constraint, Version};

/// Resolved metadata for a single package version, returned by [`Provider::metadata`].
pub struct PackageMetadata {
    /// The package this metadata belongs to.
    pub name: PackageName,
    /// The specific version this metadata describes.
    pub version: Version,
    /// Runtime dependencies: package name paired with its parsed constraint.
    pub require: Vec<(PackageName, Constraint)>,
    /// Development-only dependencies: package name paired with its parsed constraint.
    pub require_dev: Vec<(PackageName, Constraint)>,
    /// Packages this version replaces: package name paired with the replaced version.
    pub replace: Vec<(PackageName, Version)>,
    /// Virtual packages this version provides: package name paired with the provided version.
    pub provide: Vec<(PackageName, Version)>,
}

/// The async interface the resolver uses to fetch package data.
///
/// Implementors: one per kind of source in `phast-registry`, aggregated behind
/// `ChainedProvider`, which is what the resolver is actually handed;
/// `MemoryProvider` stands in for tests.
pub trait Provider: Send + Sync {
    /// Return available versions for a package, newest first.
    ///
    /// The order is the contract, not a courtesy: the solver takes the first
    /// version that fits the range it is working with, so a list out of order
    /// resolves to an older release without anything reporting it.
    ///
    /// Filtering before returning is the implementor's business. A registry drops
    /// what the manifest's `minimum_stability` excludes; a source serving one
    /// package has nothing to filter.
    fn versions(
        &self,
        name: &PackageName,
    ) -> impl Future<Output = anyhow::Result<Vec<Version>>> + Send;

    /// Return the dependency metadata for a specific package version.
    fn metadata(
        &self,
        name: &PackageName,
        version: &Version,
    ) -> impl Future<Output = anyhow::Result<PackageMetadata>> + Send;

    /// Speculatively fetch and cache metadata for a set of packages.
    ///
    /// Called once a package's dependencies are known, so they are warm in the
    /// cache before the solver asks for them. Failures are swallowed: a prefetch
    /// that fails costs a stall, never a wrong answer, so nothing may depend on
    /// it having run.
    ///
    /// The default implementation is a no-op, so a provider with nothing to warm
    /// implements nothing.
    fn prefetch(&self, _names: &[PackageName]) -> impl Future<Output = ()> + Send {
        std::future::ready(())
    }
}
