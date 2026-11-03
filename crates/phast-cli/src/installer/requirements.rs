//! Requirement list construction.
//!
//! One caller, `Resolver::resolve`, which hands over `Workspace::manifest()` and then appends
//! whatever the command proposed. In a workspace that manifest is the merged one, so the list
//! comes back as every member's requirements in a single flat set, carrying no record of
//! which member declared each. Enough to solve with, since a solve needs the constraint and
//! not its author, and not enough to attribute one.

use phast_core::manifest::Manifest;
use phast_resolver::Requirement;

/// Build a list of resolver requirements from a manifest.
///
/// When `include_dev` is `false`, `require-dev` entries are omitted. When it is true they are
/// appended rather than merged, carrying `is_dev` so the solver can tag what only a dev
/// requirement reaches. A package named in both sections therefore arrives twice, which the
/// solver intersects; `validate` is what refuses it up front.
#[rustfmt::skip]
pub fn collect(manifest: &Manifest, include_dev: bool) -> Vec<Requirement> {
    let mut requirements: Vec<Requirement> = manifest
        .require
        .iter()
        .map(|(name, spec)| Requirement {
            name: name.clone(),
            spec: spec.clone(),
            is_dev: false,
        })
        .collect();

    if include_dev {
        requirements.extend(
            manifest
                .require_dev
                .iter()
                .map(|(name, spec)| Requirement {
                    name: name.clone(),
                    spec: spec.clone(),
                    is_dev: true,
                }),
        );
    }
    requirements
}
