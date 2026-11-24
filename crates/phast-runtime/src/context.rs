//! Data passed to PHP callbacks.
//!
//! [`Project`] is built once per command and shared by every script the command runs.
//! [`Invocation`] is built for each run.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value as JsonValue;

/// What a PHP callback's bootstrap reads: the method to call, the project, and this run.
///
/// For `phast run dump` in `/var/www/acme`, the JSON looks like this. `project` is `null`
/// when the environment has none, and `extra` and `version` are `null` when the
/// manifest doesn't declare them.
///
/// ```json
/// {
///   "callback": { "class": "Acme\\Probe\\Hook", "method": "run" },
///   "project": {
///     "name": "acme/probe",
///     "version": null,
///     "extra": null,
///     "require": {},
///     "require_dev": {},
///     "autoload": {
///       "psr-4": { "Acme\\Probe\\": ["src/"] },
///       "psr-0": {},
///       "classmap": [],
///       "files": [],
///       "exclude-from-classmap": []
///     },
///     "vendor_dir": "/var/www/acme/vendor",
///     "bin_dir": "/var/www/acme/vendor/bin",
///     "dev_mode": true
///   },
///   "invocation": {
///     "event": "dump",
///     "arguments": [],
///     "rules": {
///       "psr4": { "Acme\\Probe\\": ["/var/www/acme/src/"] },
///       "psr0": {}
///     }
///   }
/// }
/// ```
#[derive(Debug, Serialize)]
pub struct CallbackContext<'a> {
    /// The static method to call.
    pub callback: Target<'a>,
    /// The project, or `null` when the environment has none.
    pub project: Option<&'a Project>,
    /// This run of the script.
    pub invocation: &'a Invocation,
}

/// A static method, as its class and its name.
#[derive(Debug, Serialize)]
pub struct Target<'a> {
    /// The class, such as `Acme\Hooks\Report`.
    pub class: &'a str,
    /// The method, such as `show`.
    pub method: &'a str,
}

/// The project every script in one command runs in.
#[derive(Debug, Clone, Serialize)]
pub struct Project {
    /// The root package's name, if it declares one.
    pub name: Option<String>,
    /// The root package's version, if it declares one.
    pub version: Option<String>,
    /// The root package's `extra`, as written, or `null` when the manifest has none.
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
