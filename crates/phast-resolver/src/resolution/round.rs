//! One solve: wiring the bridge, the dispatcher and PubGrub together, then turning what
//! PubGrub selected into the packages to install.

use crate::bridge::build_channel;
use crate::bridge::dispatcher::Dispatcher;
use crate::bridge::virtual_registry::VirtualRegistry;
use crate::provider::Provider;
use crate::provider::pubgrub::{PubGrubAdapter, PubGrubPackage};
use crate::ranges::constraint_to_range;
use crate::reporter;
use crate::resolver::ResolveError;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use phast_core::package::PackageName;
use phast_version::{Constraint, NumericVersion, Stability, Version};

use pubgrub::{Ranges, resolve as pb_resolve};

/// Facts the solve discovers as metadata streams in, recorded for `run_solve` to
/// apply once PubGrub finishes. Shared between the dependency provider (which
/// marks virtuals) and the dispatcher (which records replaces), since both run
/// concurrently with the solve but the decisions are made after it.
#[derive(Default)]
pub struct Observations {
    /// Packages confirmed absent from every registry: candidates for virtual
    /// satisfaction via `provide`, and filtered from the final resolution.
    pub virtual_packages: HashSet<PackageName>,
    /// Replaced package -> the packages that declare they replace it. A resolved
    /// package replaced by another resolved package is dropped from the output.
    pub replaced_by: HashMap<PackageName, Vec<PackageName>>,
    /// Virtual package -> the packages that declare they provide it. Read after the
    /// solve: a typo and an interface like `psr/log-implementation` are both absent
    /// from every registry, and this is what tells them apart.
    pub provided_by: HashMap<PackageName, Vec<PackageName>>,
}

// TODO: worth extracting into an Observer of its own.
/// Shared handle to the [`Observations`] for one solve.
pub type SharedObservations = Arc<Mutex<Observations>>;

/// The invariants of one resolution: everything that stays constant across the
/// dev/prod passes. Built once by [`resolve`](crate::resolve), then run with
/// each pass's root deps.
pub struct Round<P> {
    /// The package metadata provider (registry / path / vcs chain).
    pub provider: P,
    /// Platform packages (php, ext-*): environment constraints, not installs.
    pub platform: HashMap<PackageName, Version>,
    /// Preferred versions from an existing lock, to minimize churn.
    pub preferences: HashMap<PackageName, Version>,
    /// Forbidden version ranges per package, from the root `[conflict]`.
    pub conflicts: HashMap<PackageName, Ranges<Version>>,
    /// Packages the root provides/replaces, resolved to concrete versions.
    pub root_provides: Vec<(PackageName, Version)>,
}

impl<P: Provider + Clone + 'static> Round<P> {
    /// Run the PubGrub solver once for `root_deps` and return the filtered
    /// resolved package map.
    ///
    /// Wires the bridge, dependency provider, and dispatcher together, runs the
    /// solve, then filters the raw output down to the packages to install.
    #[rustfmt::skip]
    pub async fn run(
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
            Mutex::new(Observations::default()),
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
pub struct Resolved {
    /// The packages to install, once the raw selection has been filtered.
    pub packages: HashMap<PackageName, Version>,
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
        observations: &Observations,
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

/// Build a PubGrub root dependency map from a list of `(name, constraint)` pairs
/// and a set of locked pins injected as exact constraints.
///
/// When the same package appears in both the requirement list and `locked_pins`,
/// the ranges are intersected so the solver must satisfy both.
pub fn build_root_dependencies(
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
    observations: &Observations,
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
pub fn synthetic_root_version() -> Version {
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
