//! The autoload rules a manifest declares.

use std::collections::BTreeMap;

/// Every autoload rule a package declares, whichever manifest format it came from.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Autoload {
    /// PSR-4: namespace prefix -> relative path(s).
    /// Sorted: this feeds the lock and the generated autoload files,
    /// both of which must not reorder.
    pub psr4: BTreeMap<String, Vec<String>>,
    /// PSR-0: namespace prefix -> relative path(s).
    /// Sorted: this feeds the lock and the generated autoload files,
    /// both of which must not reorder.
    pub psr0: BTreeMap<String, Vec<String>>,
    /// Classmap: paths to scan for classes.
    pub classmap: Vec<String>,
    /// Files: paths to require unconditionally.
    pub files: Vec<String>,
    /// Path patterns skipped while building the classmap.
    pub exclude: Vec<String>,
}
