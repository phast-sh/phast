//! Which part of a project a command operates on.

use crate::hints;
use crate::{Member, Workspace};

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use phast_core::manifest::Manifest;
use phast_core::package::PackageName;

use anyhow::{Result, anyhow, bail};

/// The part of a project a command operates on: one workspace member, or the project itself.
///
/// A standalone project is not a separate case. It is a workspace with no members, and it
/// answers every question here from its root.
pub struct Scope<'a> {
    /// Where the command was invoked. It decides which member encloses the
    /// invocation, and stands in as the scope's directory when none does.
    dir: PathBuf,
    /// What this scope requires: borrowed from the workspace when nothing is narrowed,
    /// built from the member when something is.
    manifest: Cow<'a, Manifest>,
    /// The member narrowed to, or `None` for the project as a whole.
    member: Option<&'a Member>,
    /// The project this scope was cut from.
    workspace: &'a Workspace,
}

impl<'a> Scope<'a> {
    /// The directory this scope addresses: the member's, or where the command was invoked.
    pub fn dir(&self) -> &Path {
        self.member.map_or(self.dir.as_path(), Member::path)
    }

    /// Directory holding `phast.lock` and `vendor/`, which the whole project shares.
    pub fn root(&self) -> &Path {
        self.workspace.root()
    }

    /// The member narrowed to, if any.
    ///
    /// For commands that need more of a member than this type projects: its package
    /// identity, or its requirements.
    pub fn member(&self) -> Option<&'a Member> {
        self.member
    }

    /// Every member of the project; empty when standalone.
    ///
    /// For commands that cover the whole project rather than narrowing to one part.
    pub fn members(&self) -> &'a [Member] {
        self.workspace.members()
    }

    /// What this scope requires: the member's manifest when narrowed to one, the merged
    /// view of the whole project otherwise.
    ///
    /// A member's is expanded against its workspace, so an inherited constraint resolves.
    /// The merged view is assembled for the resolver: it folds every member's requirements
    /// together and drops the member that declared each one, and it carries the root's
    /// `[scripts]`, `[config]`, `[[repositories]]` and `[sources]` unchanged.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Whether `package` falls within this scope.
    ///
    /// A member covers its direct requirements only: every entry maps to a constraint it can
    /// change in its own `phast.toml`. Unnarrowed covers everything, which is not the same as
    /// asking the merged manifest: that holds only direct requirements, so a package reached
    /// transitively is in the project without being in it.
    pub fn includes(&self, package: &str, include_dev: bool) -> bool {
        self.member.is_none()
            || self
                .manifest
                .requires(&PackageName::new(package), include_dev)
    }
}

impl Workspace {
    /// The root as itself: the manifest it declares, not the view assembled from its members.
    ///
    /// Where [`scope`](Self::scope) answers what a part of the project requires, this answers
    /// what the root file holds: its `[config]`, `[[repositories]]` and `[sources]`, and none
    /// of the requirements its members declare.
    ///
    /// The file is read again here. A [`Workspace`] keeps the merged manifest and not the
    /// root one it was built from, so there is nothing retained to hand back.
    pub fn root_scope(&self) -> Result<Scope<'_>> {
        let manifest = Cow::Owned(Manifest::load(&self.root)?);

        Ok(Scope {
            dir: self.root.clone(),
            manifest,
            member: None,
            workspace: self,
        })
    }

    /// The part of this project an invocation in `dir` operates on.
    ///
    /// `member` is a path relative to the root, as `--member` gives it. Standing inside a
    /// member means the same as naming it, so a command behaves the same way whether the
    /// author passed the flag or ran it from the right directory.
    ///
    /// Both failures belong to the flag: it needs a workspace to name a member in,
    /// and it needs to name one that exists.
    #[rustfmt::skip]
    pub fn scope(&self, dir: &Path, member: Option<&str>) -> Result<Scope<'_>> {
        let member = self.narrow(dir, member)?;

        let manifest = match member {
            Some(m) => Cow::Owned(m.to_manifest()?),
            None    => Cow::Borrowed(self.manifest()),
        };

        Ok(Scope {
            dir: dir.to_path_buf(),
            manifest,
            member,
            workspace: self,
        })
    }

    /// Which member an invocation is narrowed to, if any.
    fn narrow(&self, dir: &Path, member: Option<&str>) -> Result<Option<&Member>> {
        // Nothing named: standing inside a member narrows to it,
        // the same as naming it.
        let Some(path) = member else {
            return Ok(self.enclosing(dir));
        };

        // Inactive means a standalone project: no `[workspace]` section,
        // so no members to name.
        if !self.is_active() {
            bail!(hints::member_requires_workspace());
        }

        // Named: a path relative to the root, so it means
        // the same from any directory.
        self.relative(Path::new(path))
            .map(Some)
            .ok_or_else(|| anyhow!("{}", hints::member_not_found(path)))
    }
}
