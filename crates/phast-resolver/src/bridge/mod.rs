//! Channel types and constructor for the async/sync bridge.
//!
//! The bridge connects PubGrub's synchronous solver (running in a
//! `spawn_blocking` thread) to the async `Provider` trait (network I/O).
//!
//! - The PubGrub thread sends a `ProviderRequest` and blocks on `response_rx.recv()`.
//! - A tokio task owns `request_rx`, awaits the provider, and sends back a `ProviderResponse`.
//!
//! [`dispatcher`] drives the other end.

pub mod dispatcher;
pub mod virtual_registry;

use crate::provider::PackageMetadata;

use std::sync::mpsc as sync_mpsc;

use phast_core::package::PackageName;
use phast_version::Version;

use tokio::sync::mpsc as async_mpsc;

/// A request sent from the PubGrub thread to the dispatcher.
pub enum ProviderRequest {
    /// Ask the provider for all known versions of a package.
    Versions { name: PackageName },
    /// Ask the provider for the dependency metadata of a specific version.
    Metadata { name: PackageName, version: Version },
}

/// A response sent from the async dispatcher back to the PubGrub thread.
pub enum ProviderResponse {
    /// Versions a provider actually serves for the requested package.
    Versions(Vec<Version>),
    /// No provider serves the package. Carries synthetic versions derived from
    /// the provides index (may be empty when no providers are known yet).
    /// The dependency provider must mark it as virtual regardless.
    VirtualVersions(Vec<Version>),
    /// Dependency metadata for a specific version.
    Metadata(PackageMetadata),
    /// A human-readable error description; the resolver will propagate this as a failure.
    Err(String),
}

/// The solver-side handle: sends requests and blocks waiting for responses.
pub struct BridgeSender {
    /// Channel for dispatching requests to the async dispatcher.
    pub(crate) request_tx: async_mpsc::UnboundedSender<ProviderRequest>,
    /// Synchronous channel on which the dispatcher sends responses back.
    pub(crate) response_rx: sync_mpsc::Receiver<ProviderResponse>,
}

/// The dispatcher-side handle: receives requests and sends responses back.
pub struct BridgeReceiver {
    /// Channel from which the async dispatcher reads incoming requests.
    pub(crate) request_rx: async_mpsc::UnboundedReceiver<ProviderRequest>,
    /// Synchronous channel used to send responses back to the solver thread.
    pub(crate) response_tx: sync_mpsc::SyncSender<ProviderResponse>,
}

/// Create a connected (sender, receiver) pair for the async/sync bridge.
///
/// Requests are unbounded because the solver only ever has one in flight, while
/// responses hold a single slot: the solver blocks on each one before asking again,
/// so a deeper buffer would never fill and would only hide that lockstep.
pub fn build_channel() -> (BridgeSender, BridgeReceiver) {
    let (request_tx, request_rx) = async_mpsc::unbounded_channel();
    let (response_tx, response_rx) = sync_mpsc::sync_channel(1);
    (
        BridgeSender {
            request_tx,
            response_rx,
        },
        BridgeReceiver {
            request_rx,
            response_tx,
        },
    )
}
