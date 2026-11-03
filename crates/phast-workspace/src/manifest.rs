//! The one manifest a workspace resolves against.
//!
//! A workspace installs as a single unit, so the solver is handed one merged manifest
//! rather than one per member. [`merge`] produces it, and along the way applies the two
//! rules that make a workspace more than a pile of projects: `{ workspace = true }` refs
//! resolve against `[workspace.dependencies]`, and two members writing the same package's
//! constraint differently is an error rather than a silent winner.

use crate::hints;
use crate::member::Member;

use std::collections::{BTreeMap, HashMap};

use phast_core::manifest::Manifest;
use phast_core::manifest::phast::{DependencySpec, PhastManifest};
use phast_version::{Constraint, Version};

use anyhow::{Context, Result, bail};
use indexmap::IndexMap;

/// Canonical version constraints defined in `[workspace.dependencies]`.
///
/// Holds the version-only table from the workspace root. Members reference
/// entries via `{ workspace = true }`, which expansion resolves before the
/// merge pass runs.
#[derive(Debug, Default)]
pub struct Dependencies {
    /// Package name to version constraint map from `[workspace.dependencies]`.
    pub(crate) versions: IndexMap<String, String>,
}

impl Dependencies {
    /// The constraint this table pins for `package`, if it pins one.
    pub fn get(&self, package: &str) -> Option<&str> {
        self.versions.get(package).map(String::as_str)
    }

    /// Replace all `{ workspace = true }` refs in `manifest` with the canonical
    /// constraint from this table.
    ///
    /// Errors if a package uses `{ workspace = true }` but has no entry here.
    pub(crate) fn expand(&self, manifest: &mut PhastManifest) -> Result<()> {
        for dependencies in [
            &mut manifest.require.dependencies,
            &mut manifest.require_dev.dependencies,
        ] {
            let to_expand: Vec<String> = dependencies
                .iter()
                .filter(|(_, spec)| matches!(spec, DependencySpec::Workspace(_)))
                .map(|(pkg, _)| pkg.clone())
                .collect();

            for package in to_expand {
                let constraint = self.versions.get(&package).ok_or_else(|| {
                    anyhow::anyhow!("{}", hints::unresolved_workspace_ref(&package))
                })?;
                dependencies.insert(package, DependencySpec::Version(constraint.clone()));
            }
        }

        Ok(())
    }
}

/// Accumulates per-package constraints during manifest merging, detecting conflicts.
///
/// Each entry tracks the constraint string and the label of the first member
/// that declared it. On a second insert with a different constraint, an error
/// is returned immediately.
struct DepConstraintTracker<'a> {
    /// Package name to `(constraint, member_label)` for the first declaration seen.
    constraints: IndexMap<String, (String, String)>,
    /// The root's `[workspace.dependencies]`, which decides what a conflict advises.
    dependencies: &'a Dependencies,
}

impl<'a> DepConstraintTracker<'a> {
    fn new(dependencies: &'a Dependencies) -> Self {
        Self {
            constraints: IndexMap::new(),
            dependencies,
        }
    }

    /// Seed an entry from the workspace root manifest.
    fn track(&mut self, package: &str, constraint: &str) {
        self.constraints.insert(
            package.to_owned(),
            (constraint.to_owned(), "(workspace root)".to_owned()),
        );
    }

    /// Record `constraint` for `package` declared by `member`, or bail on conflict.
    fn merge(&mut self, package: &str, constraint: &str, member: &str) -> Result<()> {
        match self.constraints.get(package) {
            // Nobody has asked for this package yet. Stored with who asked, so whoever
            // disagrees later can be told which member they disagree with.
            None => {
                self.constraints.insert(
                    package.to_owned(),
                    (constraint.to_owned(), member.to_owned()),
                );
            }
            // Two members want the same package spelled differently, which is a conflict
            // whether or not the ranges overlap. Which advice fixes it depends on whether
            // the root pins the package: one that is pinned wants inheriting, not pinning.
            Some((c, m)) if c != constraint => match self.dependencies.get(package) {
                Some(_) => bail!(
                    "{}",
                    hints::pinned_dependency_conflict(package, c, m, constraint, member),
                ),
                None => bail!(
                    "{}",
                    hints::dependency_conflict(package, c, m, constraint, member),
                ),
            },
            Some(_) => {}
        }
        Ok(())
    }

    /// Convert into an `IndexMap<String, DependencySpec>` for the merged manifest.
    fn into_deps(self) -> IndexMap<String, DependencySpec> {
        self.constraints
            .into_iter()
            .map(|(p, (c, _))| (p, DependencySpec::Version(c)))
            .collect()
    }
}

/// Merge all member requirements into a single manifest the resolver can process.
///
/// Intra-workspace dependencies are stripped because members are available
/// directly on disk; their source trees are registered via [`Member::autoloads`].
/// Stripped, not ignored: the constraint is checked against the sibling's declared
/// version first, so dropping the edge cannot hide a requirement it does not meet.
///
/// Returns an error naming both members and both constraints when two members
/// declare different constraints for the same external package.
pub(crate) fn merge(
    root_manifest: &PhastManifest,
    dependencies: &Dependencies,
    members: &[Member],
) -> Result<Manifest> {
    let siblings: HashMap<&str, &Member> = members
        .iter()
        .filter_map(|m| m.name().map(|n| (n, m)))
        .collect();

    let mut require = DepConstraintTracker::new(dependencies);
    let mut require_dev = DepConstraintTracker::new(dependencies);

    // Root [require]/[require-dev] are forbidden on virtual roots; these loops are no-ops.
    for (package, spec) in &root_manifest.require.dependencies {
        if let DependencySpec::Version(constraint) = spec {
            require.track(package, constraint);
        }
    }
    for (package, spec) in &root_manifest.require_dev.dependencies {
        if let DependencySpec::Version(constraint) = spec {
            require_dev.track(package, constraint);
        }
    }

    for member in members {
        let member_name = member.to_string();
        for (package, spec) in &member.manifest.require.dependencies {
            let DependencySpec::Version(constraint) = spec else {
                bail!("{}", hints::unexpanded_workspace_ref(package, &member_name));
            };
            // Intra-workspace deps are satisfied locally: validate the constraint
            // against the sibling's declared version, then exclude from the merge.
            if let Some(sibling) = siblings.get(package.as_str()) {
                validate_sibling_constraint(&member_name, package, constraint, sibling)?;
                continue;
            }
            require.merge(package, constraint, &member_name)?;
        }
        for (package, spec) in &member.manifest.require_dev.dependencies {
            let DependencySpec::Version(constraint) = spec else {
                bail!("{}", hints::unexpanded_workspace_ref(package, &member_name));
            };
            // Intra-workspace deps are satisfied locally: validate the constraint
            // against the sibling's declared version, then exclude from the merge.
            if let Some(sibling) = siblings.get(package.as_str()) {
                validate_sibling_constraint(&member_name, package, constraint, sibling)?;
                continue;
            }
            require_dev.merge(package, constraint, &member_name)?;
        }
    }

    let mut merged = PhastManifest::default();
    merged.require.dependencies = require.into_deps();
    merged.require_dev.dependencies = require_dev.into_deps();

    merged.scripts = root_manifest.scripts.clone();
    merged.config = root_manifest.config.clone();
    merged.repositories = root_manifest.repositories.clone();
    merged.sources = root_manifest.sources.clone();

    merged
        .into_manifest()
        .context("failed to convert merged workspace manifest")
}

/// Flatten expanded dependency specs to plain `name -> constraint` pairs.
///
/// Unexpanded `{ workspace = true }` refs cannot occur here: [`Workspace::load`]
/// expands them before the workspace is constructed.
///
/// [`Workspace::load`]: crate::Workspace::load
pub(crate) fn dependency_versions(
    dependencies: &IndexMap<String, DependencySpec>,
) -> BTreeMap<String, String> {
    dependencies
        .iter()
        .filter_map(|(package, spec)| match spec {
            DependencySpec::Version(constraint) => Some((package.clone(), constraint.clone())),
            DependencySpec::Workspace(_) => None,
        })
        .collect()
}

/// Check a member's constraint on a sibling against the sibling's declared version.
///
/// A depended-upon member must declare `[package].version`, and the declared
/// version must satisfy the requirer's constraint. Either failure aborts the
/// merge: a constraint that cannot be checked or is not met must not dissolve
/// silently into the merged manifest.
fn validate_sibling_constraint(
    requirer: &str,
    package: &str,
    constraint: &str,
    sibling: &Member,
) -> Result<()> {
    let Some(declared) = sibling.version() else {
        bail!("{}", hints::sibling_version_missing(requirer, package));
    };

    let parsed: Constraint = constraint.parse().with_context(|| {
        format!("invalid constraint \"{constraint}\" for {package} in {requirer}")
    })?;
    let version: Version = declared.parse().with_context(|| {
        format!("invalid version \"{declared}\" declared by workspace member {package}")
    })?;

    if !parsed.matches(&version) {
        bail!(
            "{}",
            hints::sibling_version_mismatch(requirer, package, constraint, declared),
        );
    }

    Ok(())
}
