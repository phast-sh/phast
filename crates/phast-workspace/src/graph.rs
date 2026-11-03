//! Dependency graph operations: transitive closure over the lock, and
//! topological ordering of workspace members.
//!
//! [`closure`] walks a resolved `phast.lock`, so it sees exact versions and the full
//! transitive picture.
//! [`toposort`] walks member manifests before anything is resolved, so it sees only
//! what a member wrote and only the edges that land on a sibling.

use crate::Member;
use crate::hints;

use std::collections::{HashMap, HashSet, VecDeque};

use phast_core::lock::phast::{PackageDef, PhastLock};

use anyhow::{Result, bail};

/// Compute the transitive closure of external packages needed by a workspace member.
///
/// Performs a BFS from the named member's direct `require` edges (and `require_dev`
/// when `include_dev` is true). Traversal follows only `require` edges on every
/// visited node; only the seed member's `require_dev` is part of the starting set.
/// Workspace member nodes are traversed but excluded from the result: they are
/// source trees, not installable packages.
///
/// The result is sorted by package name. Returns an empty vec when `member_name`
/// is not present in the lock.
pub fn closure(lock: &PhastLock, member_name: &str, include_dev: bool) -> Vec<PackageDef> {
    let all: HashMap<&str, &PackageDef> = lock
        .packages
        .iter()
        .chain(lock.packages_dev.iter())
        .map(|p| (p.name.as_str(), p))
        .collect();

    let Some(member) = all.get(member_name) else {
        return Vec::new();
    };

    // Tracks names already dequeued so each node is processed exactly once.
    let mut visited: HashSet<&str> = HashSet::new();
    // BFS frontier: package names whose edges have not yet been followed.
    let mut queue: VecDeque<&str> = VecDeque::new();

    visited.insert(member_name);
    for dependencies in member.require.entries.keys() {
        queue.push_back(dependencies.as_str());
    }
    if include_dev {
        for dependencies in member.require_dev.entries.keys() {
            queue.push_back(dependencies.as_str());
        }
    }

    let mut packages: Vec<PackageDef> = Vec::new();
    while let Some(package_name) = queue.pop_front() {
        // Already visited: skip, to avoid cycles and duplicate output.
        if !visited.insert(package_name) {
            continue;
        }

        // Not in the lock: platform package (e.g. "php") or unresolved optional dep.
        let Some(package) = all.get(package_name) else {
            continue;
        };

        // Enqueue runtime deps for transitive traversal.
        for dependencies in package.require.entries.keys() {
            queue.push_back(dependencies.as_str());
        }

        // Members are source trees: traverse them but don't add to the result.
        if !package.is_member() {
            packages.push((*package).clone());
        }
    }

    packages.sort_by(|a, b| a.name.cmp(&b.name));
    packages
}

/// Order workspace members so each appears after every sibling it requires.
///
/// Edges are production `require` dependencies between members; `require-dev`
/// edges are ignored, so a cycle closed only through a dev dependency is
/// permitted: a member is usable without the siblings its tests need. A cycle
/// through `require` edges is rejected.
///
/// Returns member names in dependency-first order. Members without a declared
/// `[package].name` have no identity and are omitted.
pub fn toposort(members: &[Member]) -> Result<Vec<&str>> {
    Toposort::new(member_edges(members)).run()
}

/// Build the require-edge graph: each named member mapped to the sibling members
/// it requires through production `require` (sorted, for deterministic output).
#[rustfmt::skip]
fn member_edges(members: &[Member]) -> HashMap<&str, Vec<&str>> {
    let named: HashSet<&str> = members
        .iter()
        .filter_map(|m| m.name())
        .collect();

    members
        .iter()
        .filter_map(|member| {
            let name = member.name()?;
            // Keep only require deps that name a sibling member; registry
            // packages and platform requirements (php, ext-*) are not edges.
            let mut siblings: Vec<&str> = member
                .manifest
                .require
                .dependencies
                .keys()
                .map(String::as_str)
                .filter(|dep| named.contains(dep))
                .collect();
            siblings.sort_unstable();

            Some((name, siblings))
        })
        .collect()
}

/// DFS color of a member during [`Toposort`]; an unmarked member is white.
#[derive(Clone, Copy, PartialEq)]
enum Mark {
    /// Gray: on the active DFS path. Reaching it again is a back edge: a cycle.
    Visiting,
    /// Black: fully explored and already emitted to the order.
    Explored,
}

/// A depth-first topological sort over the member require graph.
///
/// Emitting each member in post-order, after the siblings it requires, yields
/// dependency-first order. A gray member reached again is a `require` cycle.
struct Toposort<'a> {
    /// Member name -> the sibling members it requires.
    edges: HashMap<&'a str, Vec<&'a str>>,
    /// Per-member color; absent means white (not yet visited).
    mark: HashMap<&'a str, Mark>,
    /// Members in dependency-first (post-order) order.
    order: Vec<&'a str>,
    /// The active DFS path, used to reconstruct a cycle on failure.
    path: Vec<&'a str>,
}

#[rustfmt::skip]
impl<'a> Toposort<'a> {
    fn new(edges: HashMap<&'a str, Vec<&'a str>>) -> Self {
        Self {
            edges,
            mark:  HashMap::new(),
            order: Vec::new(),
            path:  Vec::new(),
        }
    }

    /// Visit every member, returning the dependency-first order or a cycle error.
    fn run(mut self) -> Result<Vec<&'a str>> {
        let mut roots: Vec<&str> = self
            .edges
            .keys()
            .copied()
            .collect();
        // Start from members in a stable order so the output, and any reported
        // cycle, are deterministic regardless of HashMap iteration order.
        roots.sort_unstable();
        for root in roots {
            self.visit(root)?;
        }

        Ok(self.order)
    }

    /// Recurse into the siblings `node` requires, then emit `node` itself.
    fn visit(&mut self, node: &'a str) -> Result<()> {
        match self.mark.get(node).copied() {
            Some(Mark::Explored) => return Ok(()),
            Some(Mark::Visiting) => bail!("{}", hints::require_cycle(&self.cycle_to(node))),
            None => {}
        }

        self.mark.insert(node, Mark::Visiting);
        self.path.push(node);

        // Required siblings are emitted first. Clone the edge list so the
        // recursive call is free to borrow `self` mutably.
        for next in self.edges.get(node).cloned().unwrap_or_default() {
            self.visit(next)?;
        }

        self.path.pop();
        self.mark.insert(node, Mark::Explored);
        self.order.push(node);

        Ok(())
    }

    /// The cycle path: from the earlier occurrence of `node` on the active path
    /// back to `node` (e.g. `["acme/a", "acme/b", "acme/a"]`).
    fn cycle_to(&self, node: &'a str) -> Vec<&'a str> {
        let start = self
            .path
            .iter()
            .position(|&n| n == node)
            .unwrap_or(0);

        let mut cycle = self.path[start..].to_vec();
        cycle.push(node);
        cycle
    }
}
