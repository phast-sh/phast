//! Filesystem primitives shared by the fixture builders.
//!
//! Both take an absolute path: resolving one against a root is the caller's job, which
//! is what [`Project`](crate::project::Project) and [`Cache`](crate::cache::Cache) do.
//! Failures panic with the path, since a fixture that cannot be staged is a broken test
//! rather than a failing assertion.

use std::fs;
use std::path::Path;

/// Read the absolute `target`.
pub(crate) fn fread(target: &Path) -> String {
    fs::read_to_string(target)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", target.display()))
}

/// Write `contents` to the absolute `target`, creating any missing parent directories.
pub(crate) fn fwrite(target: &Path, contents: &str) {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|e| panic!("failed to create {}: {e}", parent.display()));
    }

    fs::write(target, contents)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", target.display()));
}
