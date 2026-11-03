//! Virtual package registry for the dispatcher.
//!
//! A package is *virtual* when it has no installable artifact: either confirmed
//! absent from every provider, or provided/replaced by the root project. The registry
//! answers the dispatcher's two questions about such packages (which versions,
//! what metadata) and is fed facts as the solve discovers them.

use crate::provider::PackageMetadata;

use std::collections::{HashMap, HashSet};

use phast_core::package::PackageName;
use phast_version::{Constraint, NumericVersion, Version};

/// Answers the dispatcher's queries about packages with no installable artifact.
///
/// A thin front over two separate concerns: the set of packages confirmed absent
/// from every provider, and the [`ProvideGraph`] of who provides what (real packages
/// and the root). A name is virtual if either applies.
pub struct VirtualRegistry {
    /// Packages confirmed absent from every provider.
    missing: HashSet<PackageName>,
    /// Who provides each virtual package, and the synthesis built from it.
    provides: ProvideGraph,
}

impl VirtualRegistry {
    /// Create a registry seeded with the packages the root project provides or
    /// replaces (resolved to concrete versions); pass `&[]` for none.
    pub fn new(root_provides: &[(PackageName, Version)]) -> Self {
        Self {
            missing: HashSet::new(),
            provides: ProvideGraph::with_root_provides(root_provides),
        }
    }

    /// Whether `name` has no installable artifact: absent from every provider, or
    /// provided/replaced by the root.
    pub fn is_virtual(&self, name: &PackageName) -> bool {
        self.missing.contains(name) || self.provides.is_root_provided(name)
    }

    /// Record that `name` is absent from every provider.
    pub fn mark_absent(&mut self, name: PackageName) {
        self.missing.insert(name);
    }

    /// Record a real package's `provide` / `replace` declarations.
    pub fn record_provides(&mut self, name: &PackageName, provides: &[(PackageName, Version)]) {
        self.provides.record(name, provides);
    }

    /// Synthetic versions a virtual package can resolve to, one per source.
    pub fn synthetic_versions(&mut self, name: &PackageName) -> Vec<Version> {
        self.provides.synthetic_versions(name)
    }

    /// Synthetic metadata for a chosen synthetic version of a virtual package.
    pub fn synthetic_metadata(&self, name: &PackageName, version: &Version) -> PackageMetadata {
        self.provides.synthetic_metadata(name, version)
    }
}

/// What can satisfy a virtual package: a real package that declared the `provide`,
/// or the root project (via `[provide]` / `[replace]`). The root has no artifact,
/// so a root-provided package installs nothing extra.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    /// A real package whose metadata declared the `provide`.
    Package(PackageName),
    /// The root project's `[provide]` / `[replace]`.
    Root,
}

/// The graph of who provides each virtual package, and the synthesis it backs.
///
/// Built lazily as real package metadata flows through the dispatcher, and seeded
/// up front with the root's `[provide]` / `[replace]`.
struct ProvideGraph {
    /// Virtual package name -> the sources that can satisfy it, and at what version.
    providers: HashMap<PackageName, Vec<(Source, Version)>>,
    /// (virtual package name, synthetic version) -> the source that provides it.
    /// Populated in `synthetic_versions` so `synthetic_metadata` can resolve it.
    dependencies: HashMap<(PackageName, Version), Source>,
}

impl ProvideGraph {
    /// Build a graph seeded with the root's provides/replaces as `Root` sources.
    fn with_root_provides(root_provides: &[(PackageName, Version)]) -> Self {
        let mut graph = Self {
            providers: HashMap::new(),
            dependencies: HashMap::new(),
        };
        for (name, version) in root_provides {
            graph.add_source(name.clone(), Source::Root, version.clone());
        }
        graph
    }

    /// Whether the root project provides or replaces `name`.
    fn is_root_provided(&self, name: &PackageName) -> bool {
        self.providers
            .get(name)
            .is_some_and(|sources| sources.iter().any(|(source, _)| *source == Source::Root))
    }

    /// Add a real package's `provide` / `replace` declarations as `Package` sources.
    fn record(&mut self, name: &PackageName, provides: &[(PackageName, Version)]) {
        for (virtual_name, version) in provides {
            self.add_source(
                virtual_name.clone(),
                Source::Package(name.clone()),
                version.clone(),
            );
        }
    }

    /// Record that `source` can satisfy the virtual package `name` at `version`.
    fn add_source(&mut self, name: PackageName, source: Source, version: Version) {
        self.providers
            .entry(name)
            .or_default()
            .push((source, version));
    }

    /// Return synthetic versions for a virtual package, newest first, registering
    /// each in `dependencies` so `synthetic_metadata` can resolve them later.
    #[rustfmt::skip]
    fn synthetic_versions(&mut self, name: &PackageName) -> Vec<Version> {
        // Each source (a real package, or the root) yields one synthetic version.
        let Some(sources) = self.providers
            .get(name)
            .filter(|s| !s.is_empty())
        else {
            return vec![];
        };

        let mut versions = Vec::with_capacity(sources.len());
        for (i, (source, version)) in sources.iter().enumerate() {
            let synthetic = synthetic_version(version, i);
            // Remember which source this synthetic version came from: when PubGrub
            // later picks it and asks for its metadata, `synthetic_metadata` only
            // has (name, version) to go on and must recover the source from here.
            self.dependencies
                .insert((name.clone(), synthetic.clone()), source.clone());
            versions
                .push(synthetic);
        }
        versions.sort_unstable_by(|a, b| b.cmp(a));
        versions
    }

    /// Build synthetic metadata for a virtual package version.
    ///
    /// The only dependency is the real package that provides this version, or
    /// nothing when the root provides it.
    fn synthetic_metadata(&self, name: &PackageName, version: &Version) -> PackageMetadata {
        let require = match self.dependencies.get(&(name.clone(), version.clone())) {
            // Provided by a real package: depend on it so it gets installed.
            Some(Source::Package(real_name)) => vec![(real_name.clone(), Constraint::Any)],
            // Provided by the root, or unknown: nothing extra to install.
            Some(Source::Root) | None => vec![],
        };

        PackageMetadata {
            name: name.clone(),
            version: version.clone(),
            require,
            require_dev: vec![],
            provide: vec![],
            replace: vec![],
        }
    }
}

/// Build a synthetic version for the i-th provider of a virtual package.
///
/// The fourth segment carries the index, which disambiguates several providers
/// at one provided version: two sources at `1.0.0` come back as `1.0` and
/// `1.0.0.1`, the first shedding its trailing zeros on the way out. That keeps
/// every synthetic version inside any `^1.0` or `>=1.0` range.
///
/// The segment is overwritten rather than appended, so a provided `1.2.3.4` at
/// index 2 becomes `1.2.3.2`. Nothing reads that fourth segment back, and a
/// `provide` this precise is not something a range would distinguish anyway.
fn synthetic_version(version: &Version, index: usize) -> Version {
    match version {
        Version::Numeric(v) => {
            let mut segments = v.segments;
            segments[3] = index as u64;
            Version::Numeric(NumericVersion {
                segments,
                stability: v.stability,
                stability_seq: v.stability_seq,
                precision: 0,
            })
        }
        // Branch versions can't be disambiguated by segment; use as-is.
        Version::Branch(_) => version.clone(),
    }
}
