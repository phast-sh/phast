//! Where packages come from, and how they are fetched.
//!
//! Everything that reaches outside the process on a package's behalf lives here: the
//! HTTP stack, the Packagist protocol, git host APIs, the on-disk caches, and archive
//! extraction. Above this crate, the resolver and the installer see only the
//! [`Provider`](phast_resolver::provider::Provider) and [`Lockable`](provider::Lockable)
//! traits, and never learn which kind of source answered.
//!
//! ## Sources
//!
//! A package is served by exactly one provider, chosen per package by [`router`] from
//! the project's `[[repositories]]` and `[sources]`:
//!
//! - [`RegistryProvider`](provider::RegistryProvider): a Packagist-compatible API
//!   (packagist.org, Satis, Private Packagist). The common case.
//! - [`PathProvider`](provider::PathProvider): a directory on disk.
//! - [`VcsProvider`](provider::VcsProvider): a git repository, read through its host's
//!   HTTP API rather than a local clone (see [`vcs`]).
//! - [`InlineProvider`](provider::InlineProvider): a package described inline in
//!   `[sources]`, for dependencies that publish nowhere.
//!
//! [`ChainedProvider`](provider::ChainedProvider) aggregates them behind one
//! `Provider`, so the resolver asks a single question and the routing stays here.
//!
//! ## Getting the bytes
//!
//! - [`http`]: the shared client, credential resolution, the metadata cache, and the
//!   archive downloader.
//! - [`archive`]: checksum verification and extraction into `vendor/`.
//!
//! ## Packagist API
//!
//! Composer 2 uses the `metadata-url` API. Repos advertise it in
//! `packages.json` as `"metadata-url": "/p2/%package%.json"`.
//! This is the fast path. The old `provider-includes` system is
//! supported as a fallback.
//!
//! ## Cache layout
//!
//! ```text
//! {cache root}/
//!   metadata/
//!     psr/log.json               <- p2 response for the tags, stored as-is
//!     psr/log.etag               <- its ETag, for the conditional request
//!     psr/log~dev.json           <- the dev branches, a separate resource
//!     psr/log~dev.etag
//!   archives/
//!     {dist key}.zip             <- one entry per distinct archive
//! ```
//!
//! A cache scoped to a registry host inserts that host above the vendor directory,
//! `metadata/packagist.org/psr/log.json`, which keeps one registry's answer for
//! a package from overwriting another's. The dist key is whatever identifies the
//! archive: its published checksum where there is one, otherwise the commit the
//! archive was built from.

pub mod archive;
pub mod filter;
pub mod http;
pub mod p2;
pub mod provider;
pub mod router;
pub mod vcs;

mod hints;
