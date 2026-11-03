//! PubGrub-based dependency resolver for phast.
//!
//! ## Architecture
//!
//! The resolver performs no I/O. It answers one question, which versions satisfy a set
//! of requirements, by asking a [`Provider`] the caller supplies:
//!
//! - In production that is `ChainedProvider` from `phast-registry`, which routes each
//!   package to a registry, a local path, a git host, or an inline declaration. This
//!   crate never learns which.
//! - In tests it is [`provider::memory::MemoryProvider`] over
//!   hardcoded data, so the whole solver is exercised without a network.
//!
//! ## PubGrub integration
//!
//! PubGrub needs a version type that is `Ord + Clone` (`phast_version::Version`) and an
//! implementation of its own `DependencyProvider` trait. That implementation is internal
//! to this crate; callers never see it, and never implement it themselves.
//!
//! The two traits differ in more than name. PubGrub's is synchronous and its questions
//! are answered from memory; ours is async because the answers usually come from the
//! network. Bridging them is why a solve runs on a blocking thread that exchanges
//! messages with a tokio task, an approach borrowed from uv. Layered on top of that
//! translation are the PHP-specific concerns PubGrub has no notion of: platform
//! packages, stability filtering, and replace/provide aliasing.
//!
//! ## Key behaviors
//!
//! - Stability filtering: a provider drops whatever the manifest's `minimum-stability`
//!   excludes before PubGrub sees it, and a `@stability` flag on one requirement lowers
//!   that floor for that package only.
//! - Virtual packages: `provide` entries are indexed as the solve runs, so a requirement
//!   on `psr/log-implementation` is satisfied by whichever package declares it instead
//!   of failing as unresolvable.
//! - Conflicts: versions a `[conflict]` entry excludes are filtered out before a
//!   candidate goes back to PubGrub, so an excluded version is never chosen.
//! - Lock file preferences: preferred versions from an existing lock file are fed to
//!   PubGrub's `choose_version`, minimizing churn on re-resolution.

pub mod provider;
pub mod ranges;
pub mod reporter;
pub mod resolution;
pub mod resolver;

pub use provider::{PackageMetadata, Provider};
pub use resolution::Resolution;
pub use resolver::{Requirement, ResolveError, ResolveOptions, resolve};

mod bridge;
mod hints;
