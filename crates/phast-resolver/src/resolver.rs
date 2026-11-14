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

use crate::hints;
use crate::provider::Provider;
use crate::ranges::constraint_to_range;
use crate::resolution::Resolution;
use crate::resolution::round::{Round, build_root_dependencies, synthetic_root_version};

use std::collections::{HashMap, HashSet};

use phast_core::package::PackageName;
use phast_version::{Constraint, RequireSpec, Version, VersionSpec};

use pubgrub::Ranges;
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

    let resolver = Round {
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
