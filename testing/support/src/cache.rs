//! A temporary cache directory for the binary to read and write.
//!
//! Every [`Project`](crate::project::Project) already runs commands against a cache of
//! its own, so tests get isolation without asking for it. This type is for the tests
//! that treat the cache as the subject rather than the environment: they seed it, point
//! a command at it, and assert on what survived.

use crate::fs;
use crate::temp_dir;

use std::path::Path;

use tempfile::TempDir;

/// A cache directory, populated file by file.
///
/// ```no_run
/// # use phast_test_support::cache::Cache;
/// let cache = Cache::new().write("archives/vendor-alpha.zip", "not really a zip");
/// ```
pub struct Cache {
    dir: TempDir,
}

impl Cache {
    /// An empty cache directory.
    pub fn new() -> Self {
        Self { dir: temp_dir() }
    }

    /// The cache root, to hand to a command and to assert against afterwards.
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Write a file at `path`, relative to the cache root.
    ///
    /// Cache contents are opaque to the commands that report on them, which only sum
    /// file sizes and delete directories, so the bytes need not be a real archive or
    /// a real metadata document.
    pub fn write(self, path: &str, contents: &str) -> Self {
        fs::fwrite(&self.dir.path().join(path), contents);
        self
    }
}

/// Required by clippy's `new_without_default` for an argument-less `new`.
impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}
