//! Integrity verification for downloaded package archives.

use sha1::{Digest, Sha1};

/// Verify that `bytes` hash to the `expected` SHA-1 (the Packagist dist checksum).
///
/// - `Ok(())` - bytes match, or `expected` is empty. A dist derived from a VCS
///   source carries no checksum, and a registry may publish none either, so an
///   empty value passes rather than failing the install.
/// - `Err(actual)` - mismatch; the offending hex digest is returned so the
///   caller can report it and decide how to recover (e.g. drop a cache entry).
pub fn verify_sha1(bytes: &[u8], expected: &str) -> Result<(), String> {
    if expected.is_empty() {
        return Ok(());
    }

    let actual = hex::encode(Sha1::digest(bytes));
    if actual == expected {
        Ok(())
    } else {
        Err(actual)
    }
}
