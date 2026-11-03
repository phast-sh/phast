//! Topological sorting for locked package lists.

use crate::lock::phast::PackageDef;

use std::collections::{HashMap, VecDeque};

/// Sort packages so that every dependency appears before the packages that require it.
///
/// Packages with no in-set dependencies are emitted first.
/// As each package is emitted, the unresolved count of packages that depend on it
/// is decremented; packages that reach zero join the queue. Any packages involved
/// in a dependency cycle never reach zero, and are appended at the end in name order
/// like everything else.
///
/// Packages are sorted by name up front, so the result depends only on the package
/// set and its dependency edges. Without it the output inherits the caller's
/// iteration order: `enrich` walks a `HashMap`, whose order is randomized per
/// process, which would reshuffle the lock file on every run and make an unchanged
/// manifest produce a large diff. Ties (packages with no edge between them) then
/// fall out alphabetically.
#[rustfmt::skip]
pub fn kahn(mut packages: Vec<PackageDef>) -> Vec<PackageDef> {
    packages.sort_by(|a, b| a.name.cmp(&b.name));

    let n = packages.len();

    // Map each package name to its index for O(1) dependency lookups.
    let index: HashMap<&str, usize> = packages.iter().enumerate()
        .map(|(i, p)| (p.name.as_str(), i))
        .collect();

    // unsorted[current]: how many of this package's deps have not yet been emitted.
    let mut unsorted = vec![0usize; n];
    // required[dependency]: packages waiting on dependency to be emitted.
    let mut required: Vec<Vec<usize>> = vec![vec![]; n];

    for (current, pkg) in packages.iter().enumerate() {
        for dep in pkg.require.entries.keys() {
            // Skip deps absent from the index (php, ext-*, platform constraints)
            // -- only in-set packages contribute to the ordering.
            let Some(&dependency) = index.get(dep.as_str()) else {
                continue;
            };

            // current has one more unresolved dependency
            unsorted[current] += 1;
            // emitting dependency unblocks current
            required[dependency].push(current);
        }
    }

    // Seed the queue with packages that have no in-set dependencies.
    let mut queue: VecDeque<usize> = (0..n)
        .filter(|&i| unsorted[i] == 0)
        .collect();
    let mut sorted: Vec<usize> = Vec::with_capacity(n);

    while let Some(current) = queue.pop_front() {
        sorted.push(current);
        for &waiting in &required[current] {
            // one fewer unresolved dependency
            unsorted[waiting] -= 1;
            // all dependencies emitted, ready to sort
            if unsorted[waiting] == 0 {
                queue.push_back(waiting);
            }
        }
    }

    // Any packages still unsorted are part of a dependency cycle; append as-is.
    sorted.extend((0..n)
        .filter(|&i| unsorted[i] > 0));
    sorted.into_iter()
        .map(|i| packages[i].clone())
        .collect()
}
