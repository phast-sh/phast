//! The `Provider` implementations: where a package's versions and metadata come from.
//!
//! A leaf provider answers for one kind of source, and every leaf but the registry
//! serves a single package, which is why asking one to lock a name it does not own is
//! an error rather than a shrug.
//!
//! [`ChainedProvider`] sits above them holding the routing table and answers every
//! resolver call by delegating, so nothing further up learns which kind of source
//! replied.

mod chained;
mod inline;
mod path;
mod registry;
mod vcs;

pub use chained::ChainedProvider;
pub use inline::InlineProvider;
pub use path::PathProvider;
pub use registry::RegistryProvider;
pub use vcs::VcsProvider;

use std::future::Future;

use phast_core::lock::phast::PackageDef;
use phast_core::package::PackageName;
use phast_version::Version;

/// Producing the `phast.lock` entry for a resolved package.
///
/// The installer-facing counterpart to [`Provider`](phast_resolver::provider::Provider),
/// which is resolver-facing. Kept separate, and in this crate, so `phast-resolver` stays
/// free of lock-file types and its in-memory test providers carry nothing they cannot use.
///
/// Implementors: every leaf provider, and the `RoutedProvider` enum that dispatches to
/// them. Not [`ChainedProvider`], which assembles a whole `PhastLock` from the leaves
/// through `enrich` rather than producing one entry.
///
/// Both coordinates are always passed even though a single-package provider knows its
/// own package and, for path and inline sources, its only version. Uniformity is the
/// point: the caller dispatches over an enum of every provider kind, and one signature
/// is what keeps those arms from drifting.
pub trait Lockable {
    /// The lock entry for `name` at `version`.
    ///
    /// A provider that serves exactly one package errors when asked for a different
    /// one, rather than quietly locking its own under the requested name.
    fn lock(
        &self,
        name: &PackageName,
        version: &Version,
    ) -> impl Future<Output = anyhow::Result<PackageDef>> + Send;
}
