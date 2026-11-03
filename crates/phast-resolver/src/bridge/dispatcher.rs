//! Async dispatcher: drives the bridge between PubGrub and the async provider.
//!
//! A [`Dispatcher`] owns the async side of the bridge for one solve. It receives
//! requests from the PubGrub thread, delegates to the provider or the
//! `VirtualRegistry`, and sends responses back.
//!
//! After each `Metadata` response the dispatcher fires a background prefetch for
//! the package's declared dependencies so they are warm in the provider cache
//! before PubGrub asks for them.

use crate::bridge::virtual_registry::VirtualRegistry;
use crate::bridge::{BridgeReceiver, ProviderRequest, ProviderResponse, SharedObservations};
use crate::provider::Provider;

use phast_core::package::PackageName;
use phast_version::{Constraint, Version};

/// The async side of one solve: turns [`ProviderRequest`]s into [`ProviderResponse`]s,
/// backed by the real provider and the per-solve `VirtualRegistry`, recording observations
/// the resolver applies afterward.
pub struct Dispatcher<P> {
    /// The provider that actually serves packages, whatever kind of source backs it.
    provider: P,
    /// Virtual packages discovered during this solve and their provide index.
    registry: VirtualRegistry,
    /// Solve observations shared with the resolver, to which this contributes both
    /// `replaced_by` and `provided_by`.
    observations: SharedObservations,
}

#[rustfmt::skip]
impl<P: Provider + Clone + 'static> Dispatcher<P> {
    /// A dispatcher for one solve, over `provider` and `registry`.
    ///
    /// `registry` arrives already seeded, with the root's `[provide]` and `[replace]`
    /// for instance, and the dispatcher fills the rest lazily as real metadata flows
    /// through.
    pub fn new(provider: P,  registry: VirtualRegistry, observations: SharedObservations) -> Self {
        Self {
            provider,
            registry,
            observations,
        }
    }

    /// Drive the loop until the solver thread drops its sender, answering each
    /// request and sending the response back.
    pub async fn run(mut self, mut receiver: BridgeReceiver) {
        while let Some(request) = receiver.request_rx.recv().await {
            let response = match request {
                ProviderRequest::Versions { name } => {
                    self.versions(&name).await
                },
                ProviderRequest::Metadata { name, version } => {
                    self.metadata(&name, &version).await
                },
            };

            let _ = receiver.response_tx.send(response);
        }
    }

    /// Synthetic versions for a known virtual package, otherwise the provider's
    /// real versions (marking it virtual if it has none).
    async fn versions(&mut self, name: &PackageName) -> ProviderResponse {
        if self.registry.is_virtual(name) {
            return ProviderResponse::VirtualVersions(self.registry.synthetic_versions(name));
        }

        match self.provider.versions(name).await {
            Ok(versions) if !versions.is_empty() => {
                ProviderResponse::Versions(versions)
            },
            Ok(_) => {
                // No provider serves it:
                // mark absent so it is filtered from the resolution.
                self.registry.mark_absent(name.clone());
                ProviderResponse::VirtualVersions(self.registry.synthetic_versions(name))
            }
            Err(e) => {
                // `{:#}` keeps the whole chain: the outer context names the repository
                // and the inner one says why it failed, which `to_string` would drop.
                ProviderResponse::Err(format!("{e:#}"))
            },
        }
    }

    /// Synthetic metadata for a known virtual package, otherwise the provider's
    /// metadata: indexes its provide and replace, records both for the resolver,
    /// and prefetches its dependencies.
    async fn metadata(&mut self, name: &PackageName, version: &Version) -> ProviderResponse {
        if self.registry.is_virtual(name) {
            return ProviderResponse::Metadata(self.registry.synthetic_metadata(name, version));
        }

        let meta = match self.provider.metadata(name, version).await {
            Ok(m)  => m,
            Err(e) => return ProviderResponse::Err(format!("{e:#}")),
        };

        self.registry.record_provides(name, &meta.replace);
        self.registry.record_provides(name, &meta.provide);

        if !meta.replace.is_empty() { self.record_replaces(name, &meta.replace); }
        if !meta.provide.is_empty() { self.record_provides(name, &meta.provide); }
        if !meta.require.is_empty() { self.prefetch_dependencies(&meta.require); }

        ProviderResponse::Metadata(meta)
    }

    /// Record into `observations` that `name` replaces each entry in `replace`,
    /// so `run_solve` can drop a replaced package when this one is resolved.
    fn record_replaces(&self, name: &PackageName, replace: &[(PackageName, Version)]) {
        let mut observations = self
            .observations
            .lock()
            .unwrap();

        for (replaced, _) in replace {
            observations
                .replaced_by
                .entry(replaced.clone())
                .or_default()
                .push(name.clone());
        }
    }

    /// Record into `observations` that `name` provides each entry in `provide`, so the
    /// post-solve check can tell a satisfied virtual package from a name nobody carries.
    fn record_provides(&self, name: &PackageName, provide: &[(PackageName, Version)]) {
        let mut observations = self
            .observations
            .lock()
            .unwrap();

        for (provided, _) in provide {
            observations
                .provided_by
                .entry(provided.clone())
                .or_default()
                .push(name.clone());
        }
    }

    /// Spawn a background task that prefetches metadata for `dependencies`.
    ///
    /// The task is not awaited; it populates the provider's in-memory cache
    /// concurrently while PubGrub keeps solving, so the data is likely already
    /// cached by the time the solver asks for it.
    fn prefetch_dependencies(&self, dependencies: &[(PackageName, Constraint)]) {
        let names: Vec<PackageName> = dependencies
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let provider = self.provider.clone();

        tokio::spawn(async move {
            provider.prefetch(&names).await;
        });
    }
}
