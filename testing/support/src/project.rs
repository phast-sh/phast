//! A temporary project directory to run commands against.
//!
//! Tests own their fixture contents as inline constants; this only handles the
//! plumbing of getting them onto disk. The directory is removed when the [`Project`]
//! is dropped, so it must stay alive for as long as the command under test runs.

use crate::cache::Cache;
use crate::{fs, phast_cmd, temp_dir};

use std::path::Path;

use assert_cmd::Command;
use tempfile::TempDir;

/// A temporary directory, populated file by file.
///
/// ```no_run
/// # use phast_test_support::project::Project;
/// let project = Project::new()
///     .manifest("[package]\nname = \"acme/blog\"\n")
///     .lock("version = 1\n");
/// ```
pub struct Project {
    root: TempDir,
    cache: Cache,
}

impl Project {
    /// An empty project directory, with a cache of its own.
    pub fn new() -> Self {
        Self::with_cache(Cache::new())
    }

    /// An empty project directory, running commands against `cache`.
    ///
    /// For tests whose subject is the cache: seed a [`Cache`] and
    /// hand it over. Everything else takes the empty one [`new`](Project::new) makes,
    /// which is what keeps a test from reaching the developer's real cache.
    pub fn with_cache(cache: Cache) -> Self {
        Self {
            root: temp_dir(),
            cache,
        }
    }

    /// The project root, to hand to `current_dir` or a path argument.
    pub fn root(&self) -> &Path {
        self.root.path()
    }

    /// Where this project's cache lives, to assert on what a command left there.
    pub fn cache(&self) -> &Path {
        self.cache.path()
    }

    /// A command rooted at this project, using its cache.
    pub fn cmd(&self) -> Command {
        phast_cmd(self.root.path(), self.cache.path())
    }

    /// Write `phast.toml`.
    pub fn manifest(self, contents: &str) -> Self {
        self.write("phast.toml", contents)
    }

    /// Write `phast.lock`.
    pub fn lock(self, contents: &str) -> Self {
        self.write("phast.lock", contents)
    }

    /// The contents of a file at `path`, relative to the project root, as the command
    /// under test left it.
    pub fn read(&self, path: &str) -> String {
        fs::fread(&self.root.path().join(path))
    }

    /// Write a file at `path`, relative to the project root.
    ///
    /// Parent directories are created, so a workspace member can be written in one
    /// call: `file("packages/api/phast.toml", ...)`.
    pub fn write(self, path: &str, contents: &str) -> Self {
        fs::fwrite(&self.root.path().join(path), contents);
        self
    }

    /// This project's `phast.toml`, parsed, for asserting on what a command wrote.
    pub fn to_toml(&self) -> toml::Value {
        toml::from_str(&self.read("phast.toml")).expect("phast.toml is not valid TOML")
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}
