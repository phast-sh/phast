//! Presenting Phast's package sources to PubGrub.
//!
//! [`PubGrubAdapter`] implements PubGrub's `DependencyProvider`, answering its synchronous
//! questions from the async [`Provider`](super::Provider) on the other side of the
//! channel bridge. It is the inverse direction from the `Provider` implementations
//! themselves: those say where a package comes from, this one hands what they say to
//! the solver.
//!
//! ## Virtual package support
//!
//! When no provider serves a package, the dispatcher returns `VirtualVersions`.
//! The provider marks it virtual (in the shared observations) and assigns it the lowest
//! possible priority so that real packages (and their `provide` declarations) are processed
//! first. On subsequent `choose_version` calls the dispatcher may now have synthetic
//! versions from the provides_index; if not, a placeholder version derived from
//! the constraint range is returned so PubGrub never gets stuck on a virtual package.

use crate::bridge::{BridgeSender, ProviderRequest, ProviderResponse, SharedObservations};
use crate::ranges::{constraint_to_range, range_to_version};
use crate::resolver::ResolveError;

use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::sync::Mutex;

use phast_core::package::PackageName;
use phast_version::Version;

use pubgrub::{Dependencies, DependencyConstraints, PackageResolutionStatistics, Ranges};

/// PubGrub's `DependencyProvider`, answered over the channel bridge by the async
/// [`Provider`](super::Provider) the caller supplied.
pub struct PubGrubAdapter {
    /// Solver-side bridge handle used to send requests and receive responses.
    bridge: BridgeSender,
    /// Platform packages (php, ext-*) are pre-seeded with a fixed single version.
    platform: HashMap<PackageName, Version>,
    /// The version the synthetic root resolves at: the manifest's own where it
    /// declares one, and a synthetic `0.0.0.0` where it does not.
    root_version: Version,
    /// The root package's dependencies: the project requirements as resolved ranges.
    root_deps: DependencyConstraints<PubGrubPackage, Ranges<Version>>,
    /// Cache of available versions per package, fetched on first access.
    ///
    /// Only a non-empty answer is stored, so a package that came back with nothing
    /// is asked again rather than remembered as absent. A virtual package carrying
    /// synthetic versions is cached like any other.
    versions_cache: Mutex<HashMap<PackageName, Vec<Version>>>,
    /// Solve observations shared with the resolver and dispatcher. This provider
    /// writes `virtual_packages` (those no provider serves) into it; the resolver
    /// reads them, plus the dispatcher's `replaced_by`, to filter the final output.
    observations: SharedObservations,
    /// Preferred versions from the existing lock file.
    ///
    /// When a preferred version satisfies the requested range, it is returned
    /// immediately without fetching the full version list, which keeps the lock
    /// stable and saves a round-trip.
    preferences: HashMap<PackageName, Version>,
    /// Versions excluded by `[conflict]` declarations in the manifest.
    ///
    /// Maps each conflicted package to the range of versions that must not be selected.
    /// Applied in `choose_version` before returning a candidate to PubGrub.
    conflicts: HashMap<PackageName, Ranges<Version>>,
}

impl PubGrubAdapter {
    /// An adapter for one solve, answering PubGrub over `bridge`.
    ///
    /// Each argument becomes the field of the same name.
    pub fn new(
        bridge: BridgeSender,
        platform: HashMap<PackageName, Version>,
        root_version: Version,
        root_deps: DependencyConstraints<PubGrubPackage, Ranges<Version>>,
        observations: SharedObservations,
        preferences: HashMap<PackageName, Version>,
        conflicts: HashMap<PackageName, Ranges<Version>>,
    ) -> Self {
        let versions_cache = Mutex::new(HashMap::new());
        Self {
            bridge,
            platform,
            root_version,
            root_deps,
            versions_cache,
            observations,
            preferences,
            conflicts,
        }
    }

    /// Fetch available versions for `name` from the dispatcher, returning the cached result on repeat calls.
    #[rustfmt::skip]
    fn fetch_versions(&self, name: &PackageName) -> Result<Vec<Version>, ResolveError> {
        // Only serve from cache when we have a real (non-empty) result.
        if let Some(cached) = self.versions_cache.lock().unwrap().get(name)
            && !cached.is_empty()
        {
            return Ok(cached.clone());
        }

        self.bridge
            .request_tx
            .send(ProviderRequest::Versions { name: name.clone() })
            .map_err(|_| ResolveError::Conflict("bridge channel closed".into()))?;

        match self.bridge.response_rx.recv() {
            Ok(ProviderResponse::Versions(versions)) => {
                if !versions.is_empty() {
                    self.versions_cache
                        .lock()
                        .unwrap()
                        .insert(name.clone(), versions.clone());
                }
                Ok(versions)
            }
            Ok(ProviderResponse::VirtualVersions(versions)) => {
                // No provider serves it, so always mark it virtual: it must be filtered
                // from the final resolution even when providers are already known.
                self.observations
                    .lock()
                    .unwrap()
                    .virtual_packages
                    .insert(name.clone());

                if !versions.is_empty() {
                    self.versions_cache
                        .lock()
                        .unwrap()
                        .insert(name.clone(), versions.clone());
                }
                Ok(versions)
            }
            Ok(ProviderResponse::Err(e)) => {
                Err(ResolveError::Provider(anyhow::anyhow!(e)))
            },
            Ok(_) => {
                Err(ResolveError::Conflict("unexpected bridge response".into()))
            },
            Err(_) => {
                Err(ResolveError::Conflict("bridge channel closed".into()))
            },
        }
    }
}

/// Priority bands for PubGrub package resolution (all wrapped in `Reverse`).
/// Smaller value = higher priority.
const PRIORITY_ROOT: usize = 0;
const PRIORITY_PLATFORM: usize = 1;

/// Not-yet-queried real packages: below any known count but well above virtual.
const PRIORITY_UNKNOWN_REAL: usize = usize::MAX >> 1;
const PRIORITY_VIRTUAL: usize = usize::MAX;

#[rustfmt::skip]
impl pubgrub::DependencyProvider for PubGrubAdapter {
    type P        = PubGrubPackage;
    type V        = Version;
    type VS       = Ranges<Version>;
    type Priority = Reverse<usize>;
    type M        = String;
    type Err      = ResolveError;

    /// Assign a resolution priority using the `PRIORITY_*` bands:
    /// root first, then platform packages, then real packages ordered by fewest
    /// known versions (most constrained first), then virtual packages last.
    fn prioritize(
        &self,
        package: &PubGrubPackage,
        _range: &Ranges<Version>,
        _stats: &PackageResolutionStatistics,
    ) -> Self::Priority {
        match package {
            PubGrubPackage::Root => {
                Reverse(PRIORITY_ROOT)
            },
            PubGrubPackage::Package(name) if self.platform.contains_key(name) => {
                Reverse(PRIORITY_PLATFORM)
            }
            PubGrubPackage::Package(name) => {
                // Virtual packages have the lowest priority so all real packages
                // (and their `provide` declarations) are processed before we try to satisfy them.
                if self.observations
                    .lock()
                    .unwrap()
                    .virtual_packages
                    .contains(name)
                {
                    return Reverse(PRIORITY_VIRTUAL);
                }

                // Fewer known versions = more constrained = higher priority.
                // +2 keeps the result above ROOT (0) and PLATFORM (1).
                //
                // Not-yet-queried packages fall back to PRIORITY_UNKNOWN_REAL,
                // which is distinct from PRIORITY_VIRTUAL and not fed through
                // saturating_add to avoid the sentinel collision.
                let priority = match self
                    .versions_cache
                    .lock()
                    .unwrap()
                    .get(name)
                    .map(|v| v.len())
                {
                    Some(count) => count.saturating_add(2),
                    None => PRIORITY_UNKNOWN_REAL,
                };

                Reverse(priority)
            }
        }
    }

    /// Select a version of `package` within `range`, fetching from the dispatcher
    /// on first access.
    ///
    /// A locked preference wins outright when it fits the range, which is what holds
    /// a lock steady across re-resolutions. Otherwise, the first version in range is
    /// taken, once anything `[conflict]` excludes has been dropped, so the provider's
    /// newest-first ordering is what makes that the newest fit. A virtual package
    /// gets a placeholder derived from the range, so PubGrub can keep solving.
    fn choose_version(
        &self,
        package: &PubGrubPackage,
        range: &Ranges<Version>,
    ) -> Result<Option<Version>, ResolveError> {
        match package {
            PubGrubPackage::Root => Ok(range
                .contains(&self.root_version)
                .then(|| self.root_version.clone())),
            PubGrubPackage::Package(name) => {
                if let Some(v) = self.platform.get(name) {
                    return Ok(range.contains(v).then(|| v.clone()));
                }

                // Prefer the locked version when it satisfies the range. This keeps
                // the lock stable across re-resolutions (e.g. after `phast add`).
                if let Some(preferred) = self.preferences.get(name)
                    && range.contains(preferred)
                {
                    return Ok(Some(preferred.clone()));
                }

                let versions = self.fetch_versions(name)?;
                if !versions.is_empty() {
                    let allowed: Vec<Version> = match self.conflicts.get(name) {
                        Some(conflict_range) => versions
                            .into_iter()
                            .filter(|v| !conflict_range.contains(v))
                            .collect(),
                        None => versions,
                    };
                    return Ok(allowed.into_iter().find(|v| range.contains(v)));
                }

                // No provider serves the package. If it is now a known virtual,
                // synthesize a placeholder version from the range so PubGrub can
                // continue. The placeholder is filtered from the final resolution.
                if self
                    .observations
                    .lock()
                    .unwrap()
                    .virtual_packages
                    .contains(name)
                {
                    return Ok(Some(range_to_version(range)));
                }

                Ok(None)
            }
        }
    }

    /// Fetch the dependency map for `package` at `version` from the dispatcher.
    /// Returns platform packages as dependency-free and the root package with
    /// its pre-computed requirement ranges.
    fn get_dependencies(
        &self,
        package: &PubGrubPackage,
        version: &Version,
    ) -> Result<Dependencies<PubGrubPackage, Ranges<Version>, String>, ResolveError> {
        match package {
            PubGrubPackage::Root if version == &self.root_version => {
                Ok(Dependencies::Available(self.root_deps.clone()))
            }
            PubGrubPackage::Root => {
                Ok(Dependencies::Unavailable("unknown root version".into()))
            },
            PubGrubPackage::Package(name) => {
                if self.platform.contains_key(name) {
                    return Ok(Dependencies::Available(DependencyConstraints::default()));
                }

                self.bridge
                    .request_tx
                    .send(ProviderRequest::Metadata {
                        name: name.clone(),
                        version: version.clone(),
                    })
                    .map_err(|_| ResolveError::Conflict("bridge channel closed".into()))?;

                let meta = match self.bridge.response_rx.recv() {
                    Ok(ProviderResponse::Metadata(m)) => m,
                    Ok(ProviderResponse::Err(e)) => {
                        return Err(ResolveError::Provider(anyhow::anyhow!(e)));
                    }
                    Ok(_) => {
                        return Err(ResolveError::Conflict("unexpected bridge response".into()));
                    }
                    Err(_) => {
                        return Err(ResolveError::Conflict("bridge channel closed".into()));
                    }
                };

                let constraints = meta
                    .require
                    .iter()
                    .map(|(dep_name, constraint)| {
                        (
                            PubGrubPackage::Package(dep_name.clone()),
                            constraint_to_range(constraint),
                        )
                    })
                    .collect::<DependencyConstraints<_, _>>();

                Ok(Dependencies::Available(constraints))
            }
        }
    }
}

/// The package type used by PubGrub internally.
///
/// `Root` is a synthetic entry point that carries the project's requirements
/// as its dependencies. It never appears in the resolution output.
/// `Package` wraps a real `PackageName`.
///
/// Using an enum (rather than a magic string) makes it impossible to collide
/// with a real package name and lets filtering use a pattern match.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PubGrubPackage {
    /// Synthetic root package carrying the project's direct requirements.
    Root,
    /// A real package, whatever source serves it.
    Package(PackageName),
}

impl Display for PubGrubPackage {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            PubGrubPackage::Root => write!(f, "<root>"),
            PubGrubPackage::Package(name) => write!(f, "{}", name),
        }
    }
}
