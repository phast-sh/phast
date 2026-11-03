//! Platform package detection and platform map construction.
//!
//! Three sources in falling precedence: `config.platform` from the manifest, then the PHP
//! version read from the interpreter when `php` was not set there, then every loaded
//! extension. Detection only fills gaps, so a manifest entry is never overwritten.
//!
//! An entry `config.platform` cannot parse is dropped rather than refused, and detection
//! fills the hole it leaves. `php = "garbage"` alongside `require.php = "^9.0"` fails as
//! `no version of php satisfies [>=9.0, <10.0]`, naming neither the typo nor the file.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::io::Write;
use std::process::{Command, Output, Stdio};

use phast_core::manifest::Manifest;
use phast_core::package::PackageName;
use phast_runtime::php;
use phast_version::Version;
use phast_version::parser;

use anyhow::Context;

/// PHP script that prints all loaded extensions and platform virtual packages.
///
/// Output format: one `{name}={version}` per line.
///
/// In addition to `ext-{name}` entries, this script emits `php-64bit` or
/// `php-32bit` (whichever applies) at the PHP version. Composer does the same,
/// and these are the only commonly-required `php-*` virtual platform packages.
const EXTENSIONS_SCRIPT: &str = include_str!("stubs/platform.php");

/// Build the platform package map from `config.platform`, filling what it leaves out by
/// asking the interpreter: the PHP version when `php` is unset, then every loaded extension.
pub fn build_platform_map(manifest: &Manifest) -> anyhow::Result<HashMap<PackageName, Version>> {
    let mut map: HashMap<PackageName, Version> = manifest
        .config
        .platform
        .iter()
        .filter_map(|(name, version)| {
            let package = PackageName::new(name);
            let version = parser::parse_version(version).ok()?;
            Some((package, version))
        })
        .collect();

    let php = PackageName::new("php");
    if let Entry::Vacant(e) = map.entry(php) {
        let version = detect_php_version().context(
            "could not detect PHP version -- set config.platform.php in your manifest to resolve without PHP installed",
        )?;
        e.insert(version);
    }

    // Inject all loaded PHP extensions so transitive `require: {ext-dom: "*"}` from
    // dependencies are satisfied without hitting the network.
    // Entries already set in config.platform take precedence.
    for (name, version) in detect_php_extensions() {
        map.entry(name).or_insert(version);
    }

    Ok(map)
}

/// Detect the PHP version from the environment by running `php --version`.
///
/// Returns an error if PHP is not installed or its version cannot be parsed.
pub fn detect_php_version() -> anyhow::Result<Version> {
    let output = Command::new(php::binary())
        .arg("--version")
        .output()
        .context("failed to run `php --version` -- is PHP installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output looks like: "PHP x.y.z (cli) ..."
    let version_str = stdout
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("PHP "))
        .and_then(|s| s.split_whitespace().next())
        .context("unexpected output from `php --version`")?;

    parser::parse_version(version_str)
        .with_context(|| format!("could not parse PHP version '{}'", version_str))
}

/// Detect all loaded PHP extensions and their versions.
///
/// Returns an empty map on any failure (PHP not installed, script error, etc.).
/// Failures are non-fatal: missing extensions will surface as resolver errors
/// only if a dependency actually requires them.
fn detect_php_extensions() -> HashMap<PackageName, Version> {
    let Some(output) = run_extensions_script() else {
        return HashMap::new();
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut map = HashMap::new();

    for line in stdout.lines() {
        let Some((name, version_str)) = line.split_once('=') else {
            continue;
        };

        let package = PackageName::new(name.trim());
        if let Ok(version) = parser::parse_version(version_str.trim()) {
            map.insert(package, version);
        }
    }

    map
}

/// Run [`EXTENSIONS_SCRIPT`] and capture what it wrote to stdout.
///
/// The script goes in on stdin rather than through `php -r`. `-r` takes a bare
/// statement list and treats an opening `<?php` as a syntax error, while stdin
/// is read as an ordinary PHP file, so the stub stays a file an editor can check.
///
/// `None` when the interpreter cannot be started or cannot be fed, which the
/// caller turns into an empty map.
fn run_extensions_script() -> Option<Output> {
    let mut proc = Command::new(php::binary())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    // Dropping the handle closes the pipe, which is what tells PHP the script has
    // ended; without that it waits for more input and the read below never returns.
    proc.stdin
        .take()?
        .write_all(EXTENSIONS_SCRIPT.as_bytes())
        .ok()?;

    proc.wait_with_output().ok()
}
