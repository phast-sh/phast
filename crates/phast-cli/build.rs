//! Captures build-time metadata for `phast cli version`.
//!
//! Emits, as compile-time env vars:
//!     `PHAST_BUILD_EPOCH`
//!     `PHAST_COMMIT`
//!     `PHAST_COMMIT_DATE`
//!     `PHAST_TARGET`
//!     `PHAST_RUSTC`
//!
//! Anything derived from git or rustc degrades to `unknown` rather than failing, so a build
//! from a tarball, or on a machine without git, still succeeds.
//!
//! The build time is emitted as a raw Unix epoch and formatted at runtime, and it prefers
//! `SOURCE_DATE_EPOCH` over the wall clock so a release build stays reproducible.

use std::process::Command;
use std::time::UNIX_EPOCH;

/// Emit the build-metadata env vars and register the rebuild triggers.
#[rustfmt::skip]
fn main() {
    let epoch = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(epoch_now);
    export_env("PHAST_BUILD_EPOCH", &epoch.to_string());

    let commit_sha = git(&["rev-parse", "--short", "HEAD"]);
    export_env("PHAST_COMMIT",
         commit_sha
             .as_deref()
             .unwrap_or("unknown")
    );

    let commit_date = git(&["show", "-s", "--format=%cs", "HEAD"]);
    export_env("PHAST_COMMIT_DATE",
         commit_date
             .as_deref()
             .unwrap_or("unknown"),
    );

    let rustc_version = rustc(&["--version"])
        .and_then(rustc_semver);
    export_env("PHAST_RUSTC",
        rustc_version
            .as_deref()
            .unwrap_or("unknown"),
    );

    export_env("PHAST_TARGET",
        &std::env::var("TARGET")
            .unwrap_or_else(|_| "unknown".into()),
    );

    // Reproducible builds re-key the build time off this.
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    // Outside a git checkout (tarball or vendored build): nothing to watch.
    let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) else {
        return;
    };

    // Re-run when the checked-out commit changes: logs/HEAD (the HEAD reflog) moves
    // on every commit, checkout, and reset. A plain `cargo build` after a commit
    // touches no source file, so without this Cargo would not notice the new commit.
    println!("cargo:rerun-if-changed={git_dir}/logs/HEAD");
}

/// Emit a `cargo:rustc-env` line so `key` is readable via `env!` at compile time.
fn export_env(key: &str, value: &str) {
    println!("cargo:rustc-env={key}={value}");
}

/// The current time as a Unix epoch (seconds): the build-time fallback when
/// `SOURCE_DATE_EPOCH` is unset.
fn epoch_now() -> i64 {
    UNIX_EPOCH
        .elapsed()
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The version field of `rustc --version`, e.g. `1.86.0` from `rustc 1.86.0 (..)`.
#[rustfmt::skip]
fn rustc_semver(output: String) -> Option<String> {
    output
        .split_whitespace()
        .nth(1)
        .map(str::to_owned)
}

/// Run `git <args>`, returning trimmed stdout on success, `None` otherwise.
#[rustfmt::skip]
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .to_owned();
    (!text.is_empty()).then_some(text)
}

/// Run `rustc <args>`, returning trimmed stdout on success, `None` otherwise.
/// Uses the `RUSTC` Cargo sets, so it matches the compiler doing the build.
#[rustfmt::skip]
fn rustc(args: &[&str]) -> Option<String> {
    let rustc = std::env::var("RUSTC")
        .unwrap_or_else(|_| "rustc".into());
    let output = Command::new(rustc)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .to_owned();
    (!text.is_empty()).then_some(text)
}
