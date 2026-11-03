//! Materializing a downloaded package archive.
//!
//! This is post-download artifact handling, not transport, so it sits beside
//! `http` rather than inside it. Verification runs before extraction, so bytes
//! that fail their published checksum never reach the vendor directory.
//!
//! The two concerns share no code and pull in different dependencies
//! (`sha1`/`hex` vs `zip`), so they live in separate modules.

pub mod extractor;
pub mod integrity;
