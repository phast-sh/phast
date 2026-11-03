//! Flags shared by more than one command.
//!
//! Declared once and flattened in, so a flag means the same thing, reads the same way in
//! `--help`, and cannot drift into a second spelling on the seventh command that wants it.

use std::path::PathBuf;

use clap::Args;

/// Point a command at a project directory other than the one it was run from.
#[derive(Args)]
pub struct ProjectArg {
    /// Project directory to operate in; defaults to the current directory.
    #[arg(long, value_name = "DIR")]
    pub project_dir: Option<PathBuf>,
}

impl ProjectArg {
    /// The directory named, if any.
    ///
    /// Consumed by [`project::workdir`](crate::project::workdir), which supplies the
    /// current directory when nothing was named.
    pub fn dir(self) -> Option<PathBuf> {
        self.project_dir
    }
}

/// Resolve against one registry instead of the manifest's repositories.
#[derive(Args)]
pub struct RegistryArg {
    /// Override the registry URL. When absent, repositories from the manifest are used.
    #[arg(long, value_name = "URL")]
    pub registry_url: Option<String>,
}

impl RegistryArg {
    /// The registry named, if any.
    pub fn url(self) -> Option<String> {
        self.registry_url
    }
}

/// Narrow a command to one workspace member.
#[derive(Args)]
pub struct MemberArg {
    /// Workspace member to target, by its path relative to the workspace root (e.g. packages/logger).
    #[arg(long, value_name = "PATH")]
    pub member: Option<String>,
}

impl MemberArg {
    /// The member path, if one was named.
    pub fn path(&self) -> Option<&str> {
        self.member.as_deref()
    }
}

/// Leave the manifest's lifecycle hooks unrun.
#[derive(Args)]
pub struct ScriptsArg {
    /// Skip all lifecycle script hooks.
    #[arg(long)]
    pub no_scripts: bool,
}

impl ScriptsArg {
    /// Whether lifecycle hooks should run.
    ///
    /// Phrased as a skip because that is what an author types; every caller wants the
    /// question the other way round.
    pub fn run(&self) -> bool {
        !self.no_scripts
    }
}

/// Resolve classes only from the classmap.
#[derive(Args)]
pub struct ClassmapArg {
    /// Trust the classmap exclusively; drop the PSR filesystem fallback.
    #[arg(short = 'a', long)]
    pub classmap_authoritative: bool,
}

/// Leave dev dependencies out of what a command reads or installs.
#[derive(Args)]
pub struct DevArg {
    /// Exclude dev dependencies.
    #[arg(long)]
    pub no_dev: bool,
}

impl DevArg {
    /// Whether dev dependencies are included.
    ///
    /// The flag is phrased as an exclusion because that is what an author types; every
    /// caller wants the question the other way round.
    pub fn include(&self) -> bool {
        !self.no_dev
    }
}

/// Edit the manifest without installing afterwards.
///
/// Named for the flag rather than the concept, unlike its siblings here: `InstallArg` sits
/// one letter from `InstallArgs`, which is the `install` command's own arguments.
#[derive(Args)]
pub struct NoInstallArg {
    /// Update phast.toml without running install afterward.
    #[arg(long)]
    pub no_install: bool,
}

impl NoInstallArg {
    /// Whether the install that normally follows the edit is skipped.
    pub fn skip(&self) -> bool {
        self.no_install
    }
}

/// Report what a command would do, and write nothing.
#[derive(Args)]
pub struct DryRunArg {
    /// Show what would change without writing anything.
    #[arg(long)]
    pub dry_run: bool,
}
