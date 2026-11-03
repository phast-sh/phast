//! Workspace detection, member discovery, and manifest merging.
//!
//! A workspace root is a directory whose `phast.toml` contains a `[workspace]`
//! section. All other `phast.toml` files inside the member directories are
//! member manifests and do not carry a `[workspace]` section.
//!
//! The entry point for commands is [`find_root`] combined with [`Workspace::load`]:
//! 1. [`find_root`] walks up from the current directory to find the workspace root.
//! 2. [`Workspace::load`] reads the root manifest, expands member globs, and returns
//!    the assembled [`Workspace`].
//!
//! Commands that need a merged view of all dependencies call [`Workspace::manifest`].
//!
//! [`graph`] holds the questions that need the shape of the dependency graph rather
//! than its contents: which packages a single member transitively pulls in, for scoping
//! an install to one member, and what order members must be processed in, which is also
//! where a dependency cycle between members is detected.

pub mod graph;
pub mod scope;

mod discovery;
mod hints;
mod manifest;
mod member;

pub use discovery::find_root;
pub use manifest::Dependencies;
pub use member::{Member, MemberAutoload};
pub use scope::Scope;

use crate::discovery::MemberPatterns;

use std::fs::read_to_string;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use phast_core::lock::phast::PackageDef;
use phast_core::manifest::Manifest;
use phast_core::manifest::phast::PhastManifest;

use anyhow::{Context, Result, bail};

/// The resolved workspace for the current invocation.
///
/// Always present, even for standalone (single-package) projects. Use
/// [`Workspace::is_active`] to check whether workspace mode is active;
/// [`Workspace::members`] returns an empty slice for standalone projects.
#[derive(Debug, Default)]
pub struct Workspace {
    /// Absolute path to the workspace root where `phast.lock` and `vendor/` live.
    pub(crate) root: PathBuf,
    /// Pre-computed manifest: merged across all members for active workspaces,
    /// or the root manifest directly for standalone projects.
    pub(crate) manifest: Manifest,
    /// The root's `[workspace.dependencies]`; empty for standalone projects.
    /// Retained after member refs are expanded.
    pub(crate) dependencies: Dependencies,
    /// Declared workspace members; empty for standalone projects.
    pub(crate) members: Vec<Member>,
}

impl Workspace {
    /// A standalone workspace: one project, no members.
    ///
    /// Private because [`resolve`](Self::resolve) is the only thing that should build
    /// one. A `Workspace` is always present, so producing the standalone case is this
    /// crate's promise to keep.
    pub(crate) fn new(root: PathBuf, manifest: Manifest) -> Self {
        Self {
            root,
            manifest,
            ..Default::default()
        }
    }

    /// Absolute path to the workspace root where `phast.lock` and `vendor/` live.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Declared workspace members; empty for standalone projects.
    pub fn members(&self) -> &[Member] {
        &self.members
    }

    /// The resolved manifest: merged across all members for active workspaces,
    /// or the root manifest for standalone projects.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The root's `[workspace.dependencies]`: shared constraints members inherit with
    /// `{ workspace = true }`. Empty for standalone projects.
    ///
    /// Kept separate from [`manifest`](Self::manifest), which records what each member
    /// ended up requiring rather than which of those constraints the root owns.
    pub fn dependencies(&self) -> &Dependencies {
        &self.dependencies
    }

    /// Whether workspace mode is active.
    ///
    /// Having members is the whole test, because [`load`](Self::load) refuses a
    /// `[workspace]` section that declares none. A standalone project therefore
    /// never reports active, and an active one always has something to iterate.
    pub fn is_active(&self) -> bool {
        !self.members.is_empty()
    }

    /// Find a workspace member by its absolute path.
    ///
    /// Returns `None` when no member lives at exactly `path`.
    pub fn absolute(&self, path: &Path) -> Option<&Member> {
        self.members.iter().find(|m| m.path == path)
    }

    /// Find a workspace member by its path relative to the workspace root.
    ///
    /// Returns `None` when no member lives at exactly `root/path`.
    pub fn relative(&self, path: &Path) -> Option<&Member> {
        let abs = self.root.join(path);
        self.members.iter().find(|m| m.path == abs)
    }

    /// Find the member whose directory contains `path`.
    ///
    /// Returns `None` for standalone projects or when `path` does not fall inside any member.
    pub fn enclosing(&self, path: &Path) -> Option<&Member> {
        self.members.iter().find(|m| path.starts_with(&m.path))
    }

    /// The workspace containing `cwd`, whether or not one is declared.
    ///
    /// A workspace root above `cwd` is found and loaded; otherwise `cwd`'s own manifest
    /// is wrapped in a standalone workspace with no members. Commands call this when
    /// they want a uniform context and do not care which mode they are in; use
    /// [`find`](Self::find) instead when the absence of a workspace is an error.
    pub fn resolve(cwd: &Path) -> Result<Self> {
        if let Some(workspace) = Self::find(cwd)? {
            return Ok(workspace);
        }

        let manifest = Manifest::load(cwd)?;
        Ok(Self::new(cwd.to_path_buf(), manifest))
    }

    /// Find the workspace containing `start`, and load it.
    ///
    /// Walks upward for a `phast.toml` declaring `[workspace]`, then loads it.
    /// The three outcomes are distinct on purpose:
    ///
    /// - `Ok(Some(workspace))`: a root was found and loaded.
    /// - `Ok(None)`: no root above `start`. A standalone project, not an error;
    ///   the caller decides what to do instead.
    /// - `Err(_)`: a root was found but is not usable.
    pub fn find(start: &Path) -> Result<Option<Self>> {
        let Some(root) = find_root(start) else {
            return Ok(None);
        };

        Self::load(&root).map(Some)
    }

    /// Load the full workspace rooted at `root`.
    ///
    /// Expands `members` globs, reads each member `phast.toml`, merges all member
    /// requirements into a single manifest, and returns the assembled [`Workspace`].
    ///
    /// `root` must be a directory whose `phast.toml` declares `[workspace]`.
    /// Pointing this at an ordinary project directory is an error, not a fallback.
    #[rustfmt::skip]
    pub fn load(root: &Path) -> Result<Self> {
        let root_content = read_to_string(root.join("phast.toml"))
            .context("failed to read workspace root phast.toml")?;
        let root_manifest = PhastManifest::from_str(&root_content)
            .context("failed to parse workspace root phast.toml")?;

        // Establish that this is a workspace root before judging its shape: the guards
        // below tell the user how a *workspace root* must be written, which is the wrong
        // advice entirely if they simply pointed us at an ordinary project.
        let Some(workspace) = root_manifest.workspace.as_ref() else {
            bail!("{}", hints::not_a_workspace_root(root));
        };

        if !root_manifest.package.is_empty() {
            bail!("{}", hints::no_root_package_allowed());
        }
        if !root_manifest.require.is_empty() {
            bail!("{}", hints::no_root_require_allowed("require"));
        }
        if !root_manifest.require_dev.is_empty() {
            bail!("{}", hints::no_root_require_allowed("require-dev"));
        }
        if workspace.members.is_empty() {
            bail!("{}", hints::no_members_declared());
        }

        let members = MemberPatterns {
            patterns: workspace.members.clone(),
            exclude:  workspace.exclude.clone(),
        };

        let member_dirs = members.expand(root)
            .context("failed to expand workspace member patterns")?;
        if member_dirs.is_empty() {
            bail!("{}", hints::no_members_found(&members.patterns));
        }

        let mut members: Vec<Member> = member_dirs
            .into_iter()
            .map(|dir| {
                let manifest = dir.join("phast.toml");
                let content = read_to_string(&manifest)
                    .with_context(|| format!("failed to read member manifest {}", manifest.display()))?;
                let manifest = PhastManifest::from_str(&content)
                    .with_context(|| format!("failed to parse member manifest {}", manifest.display()))?;

                Ok(Member {
                    path: dir,
                    manifest,
                })
            })
            .collect::<Result<_>>()?;

        // Validate the member graph up front: members that require each other in a
        // loop (a -> b -> a, production edges only) have no valid build/resolve
        // order, so we reject them here with a clear error instead of letting the
        // later merge silently strip the offending edges. We call this purely for
        // that check; the returned topological order is not needed yet. The graph is
        // keyed on dependency names, which the workspace-ref expansion below leaves
        // untouched, so running before or after that expansion is equivalent.
        graph::toposort(&members)?;

        let dependencies = Dependencies {
            versions: workspace.dependencies.clone()
        };

        for member in &mut members {
            let manifest = member.path.join("phast.toml");
            dependencies
                .expand(&mut member.manifest)
                .with_context(|| format!("failed to expand workspace refs in {}", manifest.display()))?;
        }

        let manifest = manifest::merge(
            &root_manifest,
            &dependencies,
            &members,
        )?;

        Ok(Workspace {
            root: root.to_path_buf(),
            manifest,
            dependencies,
            members,
        })
    }

    /// Lock entries describing each named member, sorted by name.
    ///
    /// Members appear in `phast.lock` as ordinary package entries marked by a
    /// `path` relative to the workspace root. Their direct requirements are
    /// recorded as they were at lock time, including intra-workspace edges,
    /// so the lock is enough to compute per-member closures.
    ///
    /// A member without a declared `[package].name` has no package identity
    /// and is skipped. A missing version is recorded as `0.0.0`, following
    /// Cargo's default for unversioned packages.
    #[rustfmt::skip]
    pub fn lock(&self) -> Vec<PackageDef> {
        let mut packages: Vec<PackageDef> = self
            .members
            .iter()
            .filter_map(|member| {
                let name = member
                    .name()?
                    .to_owned();
                let version = member
                    .version()
                    .unwrap_or("0.0.0")
                    .to_owned();
                let path = member.path
                    .strip_prefix(&self.root)
                    .unwrap_or(&member.path)
                    .to_string_lossy()
                    .replace('\\', "/");

                let require = manifest::dependency_versions(
                    &member.manifest.require.dependencies,
                ).into();

                let require_dev = manifest::dependency_versions(
                    &member.manifest.require_dev.dependencies,
                ).into();

                Some(PackageDef {
                    name,
                    version,
                    path: Some(path),
                    require,
                    require_dev,
                    ..Default::default()
                })
            })
            .collect();

        packages.sort_by(|a, b| a.name.cmp(&b.name));
        packages
    }
}
