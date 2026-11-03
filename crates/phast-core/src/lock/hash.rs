//! Content hash computation for lock file staleness detection.
//!
//! The content hash covers only the fields that affect resolution:
//! - `require`
//! - `require-dev`
//! - `repositories`
//! - `config.platform`
//! - `config.minimum-stability`
//!
//! At a workspace root it additionally covers each member's identity and direct requirements,
//! so member version bumps, membership changes, and intra-workspace constraint edits invalidate
//! the lock. It is computed as a sha256 of the canonical JSON encoding of those fields, with
//! keys sorted. Package names are already lowercase by the time they arrive, so how they are
//! spelled in the manifest does not move the hash.
//!
//! Phast's hash is intentionally different from Composer's (which uses MD5
//! and slightly different field selection). Each tool only reads its own lock.

use crate::lock::phast::PackageDef;
use crate::manifest::Manifest;
use crate::manifest::Repository;

use std::collections::BTreeMap;

use serde_json::{Error, Value, json};
use sha2::{Digest, Sha256};

/// Compute the content hash for a manifest.
///
/// `members` carries the workspace member lock entries; pass an empty slice
/// for standalone projects, which keeps the hash input byte-identical to the
/// pre-workspace format.
///
/// Returns a hex-encoded sha256 string prefixed with `"sha256:"`.
pub fn compute(manifest: &Manifest, members: &[PackageDef]) -> Result<String, Error> {
    let canonical = canonical_json(manifest, members)?;
    let digest = Sha256::digest(canonical.as_bytes());
    Ok(format!("sha256:{}", hex::encode(digest)))
}

/// Build the canonical JSON string that is hashed.
///
/// Maps are collected into a `BTreeMap` first because `require` is a `HashMap`, whose
/// iteration order would otherwise move the hash between runs of the same manifest.
///
/// Repositories are the exception and keep the order the manifest declares them in, since
/// that order is resolution priority: reordering them is a real change, and dirties the lock.
#[rustfmt::skip]
fn canonical_json(manifest: &Manifest, members: &[PackageDef]) -> Result<String, Error> {
    let mut root = BTreeMap::new();

    // require: sorted by package name, constraint as string
    let require: BTreeMap<String, String> = manifest
        .require
        .iter()
        .map(|(k, v)| (k.as_str().to_owned(), v.to_string()))
        .collect();
    root.insert("require", serde_json::to_value(require)?);

    // require-dev: sorted by package name, constraint as string
    let require_dev: BTreeMap<String, String> = manifest
        .require_dev
        .iter()
        .map(|(k, v)| (k.as_str().to_owned(), v.to_string()))
        .collect();
    root.insert("require-dev", serde_json::to_value(require_dev)?);

    // minimum-stability as string label
    root.insert(
        "minimum-stability",
        Value::String(manifest.config.minimum_stability.as_str().to_owned()),
    );

    // platform: sorted
    let platform: BTreeMap<String, String> = manifest
        .config
        .platform
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    root.insert("platform", serde_json::to_value(platform)?);

    // repositories: serialized as an array of their URLs/types
    let repositories: Vec<Value> = manifest
        .repositories
        .iter()
        .map(repository_fingerprint)
        .collect();
    root.insert("repositories", Value::Array(repositories));

    // workspace members: sorted by name so insertion order does not matter.
    // Each entry captures identity, version, and direct requirements so that
    // renaming a member, bumping its version, or editing its constraints all
    // invalidate the lock. Standalone projects pass an empty slice and this
    // block emits nothing.
    if !members.is_empty() {
        let mut packages: Vec<&PackageDef> = members
            .iter()
            .collect();
        packages.sort_by_key(|p| p.name.as_str());

        let workspace_members: Vec<Value> = packages
            .into_iter()
            .map(member_fingerprint)
            .collect();
        root.insert("workspace-members", Value::Array(workspace_members));
    }

    serde_json::to_string(&root)
}

/// Produce a stable JSON fingerprint for a repository entry.
#[rustfmt::skip]
fn repository_fingerprint(repository: &Repository) -> Value {
    match repository {
        Repository::Vcs { url, .. }       => json!({"type": "vcs", "url": url}),
        Repository::Composer { url, .. }  => json!({"type": "composer", "url": url}),
        Repository::Path { path, .. }     => json!({"type": "path", "path": path}),
        Repository::Package { .. }        => json!({"type": "package"}),
        Repository::Packagist { url, .. } => json!({"type": "packagist", "url": url}),
    }
}

/// Produce a stable JSON fingerprint for a workspace member entry.
fn member_fingerprint(member: &PackageDef) -> Value {
    json!({
        "name":        member.name,
        "version":     member.version,
        "require":     member.require.entries.iter().collect::<BTreeMap<_, _>>(),
        "require-dev": member.require_dev.entries.iter().collect::<BTreeMap<_, _>>(),
    })
}
