//! Helpers for reading data out of a [`PhastLock`].
//!
//! The format belongs to [`PhastLock`] itself, which reads it, writes it and answers whether
//! it is still fresh. It offers no way to query one, because what a command wants to ask of a
//! lock, and the shape it wants the answer in, is a CLI concern.

use phast_core::lock::phast::{PackageDef, PhastLock};

/// Look up a package by name, searching both prod and dev sections.
///
/// Workspace members are searched too, unlike in [`collect`]. The match ignores ASCII case,
/// so `phast show PSR/Log` finds the `psr/log` entry, and the result is cloned rather than
/// borrowed so the caller is free of the lock.
pub fn find(lock: &PhastLock, name: &str) -> Option<PackageDef> {
    lock.packages
        .iter()
        .chain(lock.packages_dev.iter())
        .find(|p| p.name.eq_ignore_ascii_case(name))
        .cloned()
}

/// Collect `(name, version)` pairs from a lock file.
///
/// Always includes production packages. Dev packages are included when
/// `include_dev` is `true`. Workspace member entries are excluded: they are
/// local source trees, not installable packages, and must not be looked up
/// on a registry.
pub fn collect(lock: &PhastLock, include_dev: bool) -> Vec<(String, String)> {
    let mut packages: Vec<(String, String)> = lock
        .packages
        .iter()
        .filter(|p| !p.is_member())
        .map(|p| (p.name.clone(), p.version.clone()))
        .collect();

    if include_dev {
        packages.extend(
            lock.packages_dev
                .iter()
                .filter(|p| !p.is_member())
                .map(|p| (p.name.clone(), p.version.clone())),
        );
    }
    packages
}
