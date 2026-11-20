//! Data passed to PHP callbacks.
//!
//! [`Project`] is the same for every script a command runs.
//! [`Invocation`] describes a single run, like which event fired it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value as JsonValue;

/// The project every script in one command runs in.
#[derive(Debug, Clone, Serialize)]
pub struct Project {
    /// The root package's name, if it declares one.
    pub name: Option<String>,
    /// The root package's version, if it declares one.
    pub version: Option<String>,
    /// The root package's `extra`, as written.
    pub extra: JsonValue,
    /// The root package's requirements: package name -> constraint.
    pub require: BTreeMap<String, String>,
    /// The root package's dev requirements: package name -> constraint.
    pub require_dev: BTreeMap<String, String>,
    /// The root package's autoload rules, as written.
    pub autoload: JsonValue,
    /// Where installed packages live.
    pub vendor_dir: PathBuf,
    /// Where package binaries are linked.
    pub bin_dir: PathBuf,
    /// Whether dev packages are part of this command, which is false under `--no-dev`.
    pub dev_mode: bool,
}

/// One run of a script.
#[derive(Debug, Clone, Serialize)]
pub struct Invocation {
    /// The event that fired it, or the script's own name under `phast run`.
    pub event: String,
    /// The arguments given after `--` to `phast run <script>`. A hook never gets any.
    pub arguments: Vec<String>,
    /// What the callback's class loader registers.
    pub rules: LoaderRules,
}

/// Namespace prefixes and the directories a callback's classes load from, before the real
/// autoloader exists.
///
/// Directories are absolute.
#[derive(Debug, Clone, Default, Serialize)]
pub struct LoaderRules {
    /// PSR-4: namespace prefix -> its directories.
    pub psr4: BTreeMap<String, Vec<PathBuf>>,
    /// PSR-0: namespace prefix -> its directories.
    pub psr0: BTreeMap<String, Vec<PathBuf>>,
}
