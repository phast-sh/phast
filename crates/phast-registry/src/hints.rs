//! User-facing error messages for registry operations.
//!
//! Centralized here so all actionable registry errors use the same wording
//! and tone.

/// A request was refused by `secure-http` enforcement, which lets through only
/// `https://` and loopback hosts. Plaintext `http://` is the usual cause, not the
/// only one.
pub fn insecure_http(url: &str) -> String {
    format!(
        "refusing to send request over insecure HTTP to {url}.\n  \
         hint: use an https:// URL, or set `secure-http = false` in [config] to allow plaintext (not recommended)",
    )
}

/// A cached or freshly downloaded archive failed SHA-1 verification.
///
/// The corrupt archive has already been removed from the cache, so re-running
/// the command downloads a fresh copy.
pub fn checksum_mismatch(name: &str, version: &str, expected: &str, actual: &str) -> String {
    format!(
        "checksum mismatch for {name} {version}: expected SHA-1 {expected}, got {actual}.\n  \
         hint: the cached archive was corrupt and has been removed; re-run the command to download it again",
    )
}

/// A `path` repository's package manifest declares no name.
pub fn path_without_name(rel_path: &str) -> String {
    format!(
        "path repository '{rel_path}' declares no package name.\n  \
         hint: add a name to [package] (phast.toml) or \"name\" (composer.json)",
    )
}

/// A `path` package could not be pinned to any version: the `[sources]` entry named
/// none, its manifest declares none, and the checkout offered no branch to fall back on.
pub fn path_without_version(rel_path: &str, name: &str) -> String {
    format!(
        "path package '{name}' at '{rel_path}' declares no version.\n  \
         hint: add a version to its manifest so the dependency can be pinned",
    )
}

/// A `vcs` repository points at a host Phast does not support yet.
pub fn unsupported_vcs_host(url: &str) -> String {
    format!(
        "vcs repository '{url}' is on an unsupported host, skipping.\n  \
         hint: only github.com is supported so far",
    )
}

/// A provider serving one package was asked to lock a different one.
pub fn wrong_locked_package(served: &str, requested: &str) -> String {
    format!(
        "provider for '{served}' was asked to lock '{requested}'.\n  \
         hint: this is a routing bug; the package is pinned to the wrong source",
    )
}

/// An inline package's source host has no derivable archive, so it cannot install.
pub fn unsupported_inline_host(name: &str) -> String {
    format!(
        "inline package '{name}' is on an unsupported source host, skipping.\n  \
         hint: only github.com is supported so far",
    )
}
