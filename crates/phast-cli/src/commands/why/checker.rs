//! The reverse-dependency graph behind `phast why`: who requires a package, and the chain
//! from each requirer back to the manifest that declared it, the root in a standalone
//! project and a member in a workspace.

use std::collections::HashMap;

use phast_core::lock::phast::{PhastLock, RequireSection};
use phast_core::manifest::Manifest;
use phast_workspace::Scope;

use anyhow::Result;

/// What declared a requirement on the target.
///
/// A workspace member is neither of the other two: it is not the project being operated on,
/// and it is not something installed into `vendor/`. Telling them apart is the whole point,
/// since the merged manifest a workspace resolves against folds every member into one.
#[derive(Clone)]
pub enum Source {
    /// The project's own manifest, in a standalone project.
    Root,
    /// A workspace member, by its declared package name.
    Member(String),
    /// An installed package, by name.
    Package(String),
}

/// One entry in the reverse-dependency map: whatever requires the target.
#[derive(Clone)]
pub struct Requirer {
    /// What declared the requirement.
    pub source: Source,
    /// Constraint the requirer declared on the target.
    pub constraint: String,
    /// Whether this requirement comes from a dev context.
    pub dev: bool,
}

impl Requirer {
    /// Whether this entry is the root manifest rather than an installed package.
    fn is_root(&self) -> bool {
        matches!(self.source, Source::Root)
    }

    /// The display name: `"root (your project)"` for root, the package name otherwise.
    pub fn as_str(&self) -> &str {
        match &self.source {
            Source::Root => "root (your project)",
            Source::Member(name) | Source::Package(name) => name,
        }
    }
}

/// Node in the reverse-dependency tree for tree mode.
pub enum TreeNode {
    /// Chain ends at the root manifest.
    Root { dev: bool },
    /// An intermediate package, with its own chain toward root.
    Package {
        name: String,
        version: String,
        dev: bool,
        children: Vec<TreeNode>,
    },
    /// A cycle was detected on this path.
    Circular,
}

/// Builds and queries the reverse-dependency graph from a lock file and root manifest.
///
/// Indexes all `require` fields in the lock into a reverse map keyed by dependency name,
/// so lookups for any package are O(1) after construction.
pub struct WhyChecker {
    /// Package name → all packages (and root) that declare a `require` on it.
    required_by: HashMap<String, Vec<Requirer>>,
    /// Package name → locked version string; used to label tree nodes.
    locked_versions: HashMap<String, String>,
}

#[rustfmt::skip]
impl WhyChecker {
    /// Build the reverse-dependency graph from a lock file and the manifests `scope` covers.
    ///
    /// When `include_dev` is `false`, dev packages and dev root requirements are excluded
    /// from the graph, so queries reflect only production dependencies.
    pub fn new(scope: &Scope, lock: &PhastLock, include_dev: bool) -> Result<Self> {
        Ok(Self {
            required_by: collect_required(lock, scope, include_dev)?,
            locked_versions: collect_locked(lock),
        })
    }

    /// Query the graph for `package` and return all data needed for flat or tree rendering.
    ///
    /// Both the direct requirers and the full recursive tree are pre-computed so the
    /// caller can choose either mode without hitting the graph a second time.
    pub fn check(&self, package: &str) -> WhyResult {
        let requirers = self
            .required_by
            .get(package)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .to_vec();

        // Cycle-detection stack for the recursive DFS in collect_nodes.
        // Seeded with the queried package so that any path leading back to it
        // is detected as circular and broken with a TreeNode::Circular sentinel.
        let mut path = vec![package.to_string()];

        WhyResult {
            requirers,
            nodes: collect_nodes(
                package,
                &self.required_by,
                &self.locked_versions,
                &mut path,
            ),
        }
    }
}

/// Result of [`WhyChecker::check`]: holds all data needed for either output mode.
pub struct WhyResult {
    /// Packages (and root) that directly declare a `require` on the queried package.
    pub requirers: Vec<Requirer>,
    /// Full reverse-dependency tree: each node carries its own chain toward root.
    pub nodes: Vec<TreeNode>,
}

/// Build a map of `package_name → requirers` from the lock and the declaring manifests.
///
/// In a workspace the declaring manifests are the members, read individually: the merged
/// manifest keeps each constraint and drops the member it came from, and attributing those
/// to the root names something that cannot be true, since a root carrying `[require]` is
/// refused at load.
#[rustfmt::skip]
fn collect_required(
    lock: &PhastLock,
    scope: &Scope,
    include_dev: bool,
) -> Result<HashMap<String, Vec<Requirer>>> {
    let mut required_by: HashMap<String, Vec<Requirer>> = HashMap::new();

    for (source, manifest) in manifests(scope)? {
        for (name, constraint, dev) in requirements(&manifest, include_dev) {
            required_by
                .entry(name)
                .or_default()
                .push(Requirer {
                    source: source.clone(),
                    constraint,
                    dev,
                });
        }
    }

    for (source, require, dev) in locked(lock, include_dev) {
        for (name, constraint) in &require.entries {
            required_by
                .entry(name.to_lowercase())
                .or_default()
                .push(Requirer {
                    source: source.clone(),
                    constraint: constraint.clone(),
                    dev,
                });
        }
    }

    Ok(required_by)
}

/// Build a `name → version` lookup from all locked packages.
fn collect_locked(lock: &PhastLock) -> HashMap<String, String> {
    lock.packages
        .iter()
        .chain(lock.packages_dev.iter())
        .map(|p| (p.name.to_lowercase(), p.version.clone()))
        .collect()
}

/// Recursively build tree nodes for `package`, tracking `path` to detect cycles.
///
/// `path` holds the current ancestor chain. A requirer already present in `path`
/// is emitted as `TreeNode::Circular` instead of recursing, preventing infinite loops.
#[rustfmt::skip]
fn collect_nodes(
    package: &str,
    required_by: &HashMap<String, Vec<Requirer>>,
    locked: &HashMap<String, String>,
    path: &mut Vec<String>,
) -> Vec<TreeNode> {
    let Some(requirers) = required_by.get(package) else {
        return vec![];
    };

    let mut nodes = Vec::new();

    for requirer in requirers {
        // Root manifest is always a leaf - no further chain to follow.
        if requirer.is_root() {
            nodes.push(TreeNode::Root { dev: requirer.dev });
            continue;
        }

        let package = requirer.as_str().to_lowercase();
        // Package already on the current path: emit a sentinel to break the cycle.
        if path.contains(&package) {
            nodes.push(TreeNode::Circular);
            continue;
        }

        let version = locked
            .get(&package)
            .cloned()
            .unwrap_or_default();

        path.push(package.clone());
        let children = collect_nodes(&package, required_by, locked, path);
        path.pop();

        nodes.push(TreeNode::Package {
            name: requirer.as_str().to_owned(),
            version,
            dev: requirer.dev,
            children,
        });
    }
    nodes
}

/// The manifests declaring this project's own requirements, and what to attribute them to.
///
/// With `--member`, only that member's manifest: what a sibling requires does not answer
/// what this member requires. At a workspace root, every member's, read one at a time so each
/// requirement keeps the name of whoever declared it. Standalone, the project's own manifest,
/// where the root really is the requirer.
fn manifests(scope: &Scope) -> Result<Vec<(Source, Manifest)>> {
    // Narrowed to one member: the graph is rooted there, so its siblings are not in it.
    // Its manifest is the one the scope already carries.
    if let Some(member) = scope.member() {
        return Ok(vec![(
            Source::Member(member.to_string()),
            scope.manifest().clone(),
        )]);
    }

    // No members at all: a standalone project, where the root really is the requirer.
    if scope.members().is_empty() {
        return Ok(vec![(Source::Root, scope.manifest().clone())]);
    }

    // A workspace root, which requires nothing itself. Each member is read on its own
    // so its requirements keep its name; the merged manifest would have dropped them.
    scope
        .members()
        .iter()
        .map(|member| Ok((Source::Member(member.to_string()), member.to_manifest()?)))
        .collect()
}

/// Every requirement `manifest` declares, as `(name, constraint, dev)`.
///
/// The two sections differ only in that flag, so they are chained rather than walked
/// twice. Names are lowercased here because that is the key the map is built on.
fn requirements(
    manifest: &Manifest,
    include_dev: bool,
) -> impl Iterator<Item = (String, String, bool)> {
    let require = manifest
        .require
        .iter()
        .map(|(name, spec)| (name.as_str().to_lowercase(), spec.to_string(), false));

    let require_dev = manifest
        .require_dev
        .iter()
        .map(|(name, spec)| (name.as_str().to_lowercase(), spec.to_string(), true));

    require.chain(include_dev.then_some(require_dev).into_iter().flatten())
}

/// Every locked package, with its `require` table and whether it is dev-only.
///
/// A package in `packages_dev` is reachable only through a dev requirement, so everything
/// it requires is dev too, which is why the flag belongs to the package rather than to the
/// entry.
fn locked(
    lock: &PhastLock,
    include_dev: bool,
) -> impl Iterator<Item = (Source, &RequireSection, bool)> {
    // Members are recorded in the lock too, and their requirements are already read from
    // their own manifests above. Counting the entry as well would list them twice.
    let packages = lock
        .packages
        .iter()
        .filter(|p| !p.is_member())
        .map(|p| (Source::Package(p.name.clone()), &p.require, false));

    let packages_dev = lock
        .packages_dev
        .iter()
        .filter(|p| !p.is_member())
        .map(|p| (Source::Package(p.name.clone()), &p.require, true));

    packages.chain(include_dev.then_some(packages_dev).into_iter().flatten())
}
