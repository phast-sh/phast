//! Locating the PHP interpreter Phast shells out to.

/// The PHP binary to use for script execution and platform detection.
///
/// Checks `PHAST_PHP_BIN` first, falls back to `php` on PATH.
///
/// Only an unset variable falls back. Setting it to the empty string is taken at
/// face value and yields no program to run, so the failure surfaces at spawn time
/// rather than here.
pub fn binary() -> String {
    std::env::var("PHAST_PHP_BIN").unwrap_or_else(|_| "php".to_owned())
}
