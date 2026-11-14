//! The top-level `resolve` function.
//!
//! Entry point for `phast install` and `phast update`. Takes a root set of
//! (package, constraint) requirements, a provider, and optional locked versions,
//! and returns a `Resolution`.
//!
//! ## What a lock contributes
//!
//! Every mode runs the same solve; a lock never bypasses one. Without a lock there is
//! nothing to steady the result. With one, its versions arrive as `preferences` and are
//! taken wherever they still fit. `phast update` goes further and turns the packages it
//! was not asked to update into exact `locked_pins`, leaving only the named ones free
//! to move.

use crate::bridge::build_channel;
use crate::bridge::dispatcher::Dispatcher;
use crate::bridge::virtual_registry::VirtualRegistry;
use crate::bridge::{SharedObservations, SolveObservations};
use crate::hints;
use crate::provider::Provider;
use crate::provider::pubgrub::{PubGrubAdapter, PubGrubPackage};
use crate::ranges::constraint_to_range;
use crate::reporter;
use crate::resolution::Resolution;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use phast_core::package::PackageName;
use phast_version::{Constraint, NumericVersion, RequireSpec, Stability, Version, VersionSpec};

use pubgrub::{Ranges, resolve as pb_resolve};
use thiserror::Error;

/// Why a resolution produced no `Resolution`.
#[derive(Debug, Error)]
pub enum ResolveError {
    /// No set of versions satisfies every constraint; carries the rendered conflict tree.
    #[error("[dependency conflict] {0}")]
    Conflict(String),

    /// The solve asked for a package and no source could answer.
    #[error("[package not found] {0}")]
    NotFound(String),

    /// Requirements that ended up with nothing behind them. Carries the names rather
    /// than the message so callers can report them their own way.
    #[error("{}", hints::unresolved_requirements(.0))]
    Unresolved(Vec<String>),

    /// A provider failed to answer, so the solve could not continue.
    #[error("[provider error] {0}")]
    Provider(#[from] anyhow::Error),

    /// The solver itself misbehaved, a panicked task for instance, rather than the
    /// requirements being unsatisfiable.
    #[error("[internal error] {0}")]
    Internal(String),
}

/// Per-call configuration for [`resolve`].
#[derive(Debug, Default)]
pub struct ResolveOptions {
    /// The root manifest's declared version, if any. Root `[provide]`/`[replace]`
    /// specs resolve against it: `self.version` and `*` become this version (the
    /// monorepo-root case). `None` falls back to the synthetic root version.
    pub root_version: Option<Version>,
    /// Packages pinned to an exact version (used by `phast update` to hold non-targeted packages steady).
    pub locked_pins: HashMap<PackageName, Version>,
    /// Preferred versions from an existing lock file (used to minimize churn on stale re-resolution).
    pub preferences: HashMap<PackageName, Version>,
    /// Virtual packages this project declares it provides in `[provide]`, as
    /// version specs (`self.version`, `*`, or an exact version).
    pub provide: HashMap<PackageName, VersionSpec>,
    /// Packages this project declares it replaces in `[replace]`, as
    /// version specs (`self.version`, `*`, or an exact version).
    pub replace: HashMap<PackageName, VersionSpec>,
    /// Packages declared incompatible with this project in `[conflict]`, as
    /// specs resolved against the root version (so `self.version` is honored).
    ///
    /// Versions of these packages that fall within the resolved constraint are
    /// excluded from `choose_version` so the solver never selects them.
    /// If a transitive dep strictly requires a conflicting version, resolution fails.
    pub conflict: HashMap<PackageName, RequireSpec>,
    /// Package names known to be dev-only in the previous lock file.
    ///
    /// Used to filter `locked_pins` in no-dev solves: dev packages must not be
    /// injected as root-level requirements into a prod-only resolution.
    pub dev_package_names: HashSet<PackageName>,
}

/// A direct dependency requirement passed to the resolver.
pub struct Requirement {
    /// The package name, e.g. `psr/log`.
    pub name: PackageName,
    /// The manifest value, e.g. `^3.0` or `self.version`. Resolved against the
    /// root version inside `resolve`.
    pub spec: RequireSpec,
    /// Whether this requirement originates from `require-dev`.
    ///
    /// The resolver uses this to populate `Resolution::dev_package_names`:
    /// packages that are only reachable via dev requirements are tagged as dev.
    pub is_dev: bool,
}

/// Resolve a set of requirements against a provider.
///
/// Pass `ResolveOptions::default()` for a fresh install.
///
/// ## Dev/prod split
///
/// Requirements with `is_dev = true` trigger a two-pass resolution:
/// 1. Full pass (prod + dev): produces the actual lock content and warms the metadata cache.
/// 2. Prod-only pass: re-solves without dev requirements against the now-warm cache (no network I/O).
///
/// Packages present only in the full set are tagged as dev in `Resolution::dev_package_names`.
/// A package reachable via both prod and dev graphs stays in prod.
#[rustfmt::skip]
pub async fn resolve(
    requirements: Vec<Requirement>,
    provider: impl Provider + Clone + 'static,
    platform: HashMap<PackageName, Version>,
    options: ResolveOptions,
) -> Result<Resolution, ResolveError> {
    let ResolveOptions {
        root_version,
        locked_pins,
        preferences,
        provide,
        replace,
        conflict,
        dev_package_names,
    } = options;

    // The root's [provide] and [replace] both make the named package virtual,
    // satisfied by the root at the declared version: provide for a virtual
    // interface, replace to shadow a real package. Specs resolve against the root's
    // own version (self.version and * become it, the monorepo-root case), falling
    // back to the synthetic root version when the manifest declares none.
    // Mechanically identical for suppression, so they seed one list.
    let version = root_version
        .unwrap_or_else(synthetic_root_version);
    let root_provides: Vec<(PackageName, Version)> = provide
        .into_iter()
        .chain(replace)
        .map(|(name, spec)| (name, spec.resolve(&version)))
        .collect();
    let (prod_reqs, dev_reqs): (Vec<Requirement>, Vec<Requirement>) =
        requirements.into_iter().partition(|r| !r.is_dev);

    // require/require-dev/conflict resolve their specs against the same root
    // version as replace/provide above, so `self.version` in the root binds to it.
    let prod_pairs: Vec<(PackageName, Constraint)> = prod_reqs
        .into_iter()
        .map(|r| (r.name, r.spec.resolve(&version)))
        .collect();
    let dev_pairs: Vec<(PackageName, Constraint)> = dev_reqs
        .into_iter()
        .map(|r| (r.name, r.spec.resolve(&version)))
        .collect();

    let conflicts: HashMap<PackageName, Ranges<Version>> = conflict
        .into_iter()
        .map(|(name, spec)| (name, constraint_to_range(&spec.resolve(&version))))
        .collect();

    let resolver = Resolver {
        provider,
        platform,
        preferences,
        conflicts,
        root_provides,
    };

    if dev_pairs.is_empty() {
        // No dev requirements: single pass, dev_package_names is always empty.
        // Filter locked_pins to exclude dev packages from the previous lock so they
        // are not injected as root-level requirements into this prod-only solve.
        let prod_pins: HashMap<PackageName, Version> = locked_pins
            .into_iter()
            .filter(|(name, _)| !dev_package_names.contains(name))
            .collect();
        let resolved = resolver
            .run(build_root_dependencies(prod_pairs, &prod_pins))
            .await?;

        return Ok(Resolution {
            packages: resolved.packages,
            dev_package_names: HashSet::new(),
        });
    }

    // Two-pass resolution:
    //
    // Pass 1: full (prod + dev) with locked_pins. This is the authoritative
    // solve that produces the actual lock content. Running it first warms the
    // provider's in-memory metadata cache for all packages.
    //
    // Pass 2: prod-only, no locked_pins. Locked pins are omitted because they
    // may reference dev-only packages; injecting them would pull those packages
    // into the prod solver and corrupt the dev/prod split. Because pass 1 already
    // fetched metadata for every package, this pass incurs zero additional network I/O,
    // it is a pure CPU re-solve against the warm cache.
    let all_pairs: Vec<(PackageName, Constraint)> =
        prod_pairs.iter().cloned().chain(dev_pairs).collect();

    let resolved = resolver
        .run(build_root_dependencies(all_pairs, &locked_pins))
        .await?;
    let prod_packages = resolver
        .run(build_root_dependencies(prod_pairs, &HashMap::new()))
        .await?
        .packages;

    let dev_package_names: HashSet<PackageName> = resolved
        .packages
        .keys()
        .filter(|n| !prod_packages.contains_key(*n))
        .cloned()
        .collect();

    Ok(Resolution {
        packages: resolved.packages,
        dev_package_names,
    })
}

/// Build a PubGrub root dependency map from a list of `(name, constraint)` pairs
/// and a set of locked pins injected as exact constraints.
///
/// When the same package appears in both the requirement list and `locked_pins`,
/// the ranges are intersected so the solver must satisfy both.
fn build_root_dependencies(
    mut requirements: Vec<(PackageName, Constraint)>,
    locked_pins: &HashMap<PackageName, Version>,
) -> pubgrub::Map<PubGrubPackage, Ranges<Version>> {
    for (name, version) in locked_pins {
        requirements.push((name.clone(), Constraint::Exact(version.clone())));
    }

    let mut root_dependencies: pubgrub::Map<PubGrubPackage, Ranges<Version>> =
        pubgrub::Map::default();

    for (name, constraint) in requirements {
        let range = constraint_to_range(&constraint);
        root_dependencies
            .entry(PubGrubPackage::Package(name))
            .and_modify(|existing| *existing = existing.intersection(&range))
            .or_insert(range);
    }
    root_dependencies
}

/// The invariants of one resolution: everything that stays constant across the
/// dev/prod passes. Built once by [`resolve`], then run with each pass's root deps.
struct Resolver<P> {
    /// The package metadata provider (registry / path / vcs chain).
    provider: P,
    /// Platform packages (php, ext-*): environment constraints, not installs.
    platform: HashMap<PackageName, Version>,
    /// Preferred versions from an existing lock, to minimize churn.
    preferences: HashMap<PackageName, Version>,
    /// Forbidden version ranges per package, from the root `[conflict]`.
    conflicts: HashMap<PackageName, Ranges<Version>>,
    /// Packages the root provides/replaces, resolved to concrete versions.
    root_provides: Vec<(PackageName, Version)>,
}

impl<P: Provider + Clone + 'static> Resolver<P> {
    /// Run the PubGrub solver once for `root_deps` and return the filtered
    /// resolved package map.
    ///
    /// Wires the bridge, dependency provider, and dispatcher together, runs the
    /// solve, then filters the raw output down to the packages to install.
    #[rustfmt::skip]
    async fn run(
        &self,
        root_dependencies: pubgrub::Map<PubGrubPackage, Ranges<Version>>,
    ) -> Result<Resolved, ResolveError> {
        let root_version = synthetic_root_version();
        let (bridge_tx, bridge_rx) = build_channel();

        // The names this solve must account for, taken before the map moves into the
        // adapter below. Platform packages are left out: they constrain the environment
        // and are never selected, so their absence from the result means nothing.
        let declared: Vec<PackageName> = root_dependencies
            .keys()
            .filter_map(package_name)
            .filter(|name| !self.platform.contains_key(*name))
            .cloned()
            .collect();

        // Facts discovered during the solve: virtual packages (written by the dependency
        // provider) and replace relationships (written by the dispatcher). Read after
        // the solve to filter the output.
        let observations: SharedObservations = Arc::new(
            Mutex::new(SolveObservations::default()),
        );

        // Seed the registry with the root's [provide]/[replace] before the solve; the
        // dispatcher fills the rest lazily from real metadata.
        let dispatcher = Dispatcher::new(
            self.provider.clone(),
            VirtualRegistry::new(&self.root_provides),
            Arc::clone(&observations),
        );

        let task = tokio::spawn(dispatcher.run(bridge_rx));

        let adapter = PubGrubAdapter::new(
            bridge_tx,
            self.platform.clone(),
            root_version.clone(),
            root_dependencies.into_iter().collect(),
            Arc::clone(&observations),
            self.preferences.clone(),
            self.conflicts.clone(),
        );
        let selected = run_pubgrub(adapter, root_version)
            .await?;

        // PubGrub has finished; the dispatcher is no longer needed.
        task.abort();

        // The dispatcher has stopped, so we are the only holder;
        // let's take the facts it accumulated (virtual packages, replaces)
        // to filter the raw selection.
        let observations = observations
            .lock()
            .unwrap();

        let resolved = Resolved::new(selected, &self.platform, &observations);
        resolved.verify(&declared)?;

        Ok(resolved)
    }
}

/// What one solver pass produced.
///
/// `packages` is what gets installed. `satisfied_by` explains the names that are not
/// in it but were still asked for: a virtual package carried by whatever provides it,
/// or a package shadowed by whatever replaces it.
struct Resolved {
    /// The packages to install, once the raw selection has been filtered.
    packages: HashMap<PackageName, Version>,
    /// Absent name -> the packages declaring they provide or replace it.
    satisfied_by: HashMap<PackageName, Vec<PackageName>>,
}

impl Resolved {
    /// Filter the raw selection down to what installs, and index who carries the rest.
    ///
    /// `provide` and `replace` are merged because they answer one question here: a name
    /// missing from the resolution is fine when something selected carries it, and both
    /// declarations are ways of carrying it.
    fn new(
        selected: Vec<(PubGrubPackage, Version)>,
        platform: &HashMap<PackageName, Version>,
        observations: &SolveObservations,
    ) -> Self {
        let mut satisfied_by = observations.provided_by.clone();

        for (name, replacers) in &observations.replaced_by {
            satisfied_by
                .entry(name.clone())
                .or_default()
                .extend(replacers.iter().cloned());
        }

        Self {
            packages: filter_resolution(selected, platform, observations),
            satisfied_by,
        }
    }

    /// Fail when a declared requirement ended up with nothing behind it.
    ///
    /// When no source offers a package under that name, the solve is handed a synthetic
    /// version so it can carry on, and the name is dropped from the result. That is what
    /// lets `psr/log-implementation` resolve against whichever logger provides it, and it
    /// is also what swallows a typo. The two are only distinguishable here, once every
    /// `provide` and `replace` is known.
    fn verify(&self, declared: &[PackageName]) -> Result<(), ResolveError> {
        let unresolved: Vec<String> = declared
            .iter()
            .filter(|name| !self.packages.contains_key(*name))
            .filter(|name| !self.is_satisfied(name))
            .map(|name| name.as_str().to_owned())
            .collect();

        match unresolved.is_empty() {
            true => Ok(()),
            false => Err(ResolveError::Unresolved(unresolved)),
        }
    }

    /// Whether a package that provides or replaces `name` was itself selected.
    ///
    /// Declaring it is not enough: a provider nobody installed satisfies nothing.
    fn is_satisfied(&self, name: &PackageName) -> bool {
        self.satisfied_by.get(name).is_some_and(|carriers| {
            carriers
                .iter()
                .any(|carrier| self.packages.contains_key(carrier))
        })
    }
}

/// Run PubGrub on a blocking thread (it must not stall the async executor) and
/// translate its errors into [`ResolveError`]. Returns the raw `(package, version)`
/// selections, including root/platform/virtual entries that `filter_resolution` drops.
#[rustfmt::skip]
async fn run_pubgrub(
    adapter: PubGrubAdapter,
    root_version: Version,
) -> Result<Vec<(PubGrubPackage, Version)>, ResolveError> {
    let selected = tokio::task::spawn_blocking(move || {
        pb_resolve(&adapter, PubGrubPackage::Root, root_version).map_err(|e| match e {
            // No version selection satisfies all constraints: render the conflict tree.
            pubgrub::PubGrubError::NoSolution(mut e) =>
                ResolveError::Conflict(reporter::report(&mut e)),

            // Our `should_cancel` hook returned an error; pass it through unchanged.
            pubgrub::PubGrubError::ErrorInShouldCancel(e) => e,

            // Our `choose_version` returned an error; pass it through unchanged.
            pubgrub::PubGrubError::ErrorChoosingVersion { source, .. } => source,

            // Fetching a package's dependencies failed (network or provider error).
            pubgrub::PubGrubError::ErrorRetrievingDependencies { package, version, source } =>
                ResolveError::Provider(anyhow::anyhow!(
                    "failed to get dependencies for {package}@{version}: {source}"
                )),
        })
    })
    .await
    .map_err(|e| ResolveError::Internal(format!("solver task panicked: {e}")))??;

    Ok(
        selected
            .into_iter()
            .collect(),
    )
}

/// Reduce PubGrub's raw selections to the packages to install: drop the synthetic
/// root, platform packages, virtual packages, and any package replaced by another
/// resolved package (it is provided by that replacer).
#[rustfmt::skip]
fn filter_resolution(
    selected: Vec<(PubGrubPackage, Version)>,
    platform: &HashMap<PackageName, Version>,
    observations: &SolveObservations,
) -> HashMap<PackageName, Version> {
    // Names PubGrub selected:
    // used to test whether a package's replacer is resolved.
    let selected_names: HashSet<&PackageName> = selected
        .iter()
        .filter_map(|(package, _)| package_name(package))
        .collect();

    let mut resolved = HashMap::new();
    for (package, version) in &selected {
        let name = match package {
            // The synthetic root is never installed.
            PubGrubPackage::Root => continue,

            // Platform packages (php, ext-*) are environment constraints, not installs.
            PubGrubPackage::Package(name) if platform.contains_key(name) => {
                continue;
            },

            // Virtual packages have no artifact; a real provider satisfies them.
            PubGrubPackage::Package(name) if observations.virtual_packages.contains(name) => {
                continue;
            },

            // Replaced by a co-resolved package, which provides it: don't install it.
            PubGrubPackage::Package(name) if observations.replaced_by.get(name).is_some_and(|replacers| {
                replacers
                    .iter()
                    .any(|replacer| selected_names.contains(replacer))
            }) => {
                continue;
            },

            // A real package to install.
            PubGrubPackage::Package(name) => name.clone(),
        };
        resolved.insert(name, version.clone());
    }

    resolved
}

/// The synthetic version of the root package. Never appears in the output.
///
/// Used as the PubGrub root's version, and as the fallback the root's
/// `[provide]`/`[replace]` specs resolve against when the manifest declares no
/// version of its own.
fn synthetic_root_version() -> Version {
    Version::Numeric(NumericVersion {
        segments: [0, 0, 0, 0],
        stability: Stability::Stable,
        stability_seq: 0,
        precision: 0,
    })
}

/// The real package name of a PubGrub package, or `None` for the synthetic root.
fn package_name(package: &PubGrubPackage) -> Option<&PackageName> {
    match package {
        PubGrubPackage::Package(name) => Some(name),
        PubGrubPackage::Root => None,
    }
}
