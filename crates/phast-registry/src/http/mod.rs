//! The HTTP transport for package metadata and archives.
//!
//! The downloader owns the pipeline and its policy, when to verify and what to do
//! with a cache entry that fails, while the hashing and unzipping it calls out to
//! sit in `archive`.

pub mod auth;
pub mod cache;
pub mod client;
pub mod downloader;
