//! Reporting what declares a requirement on one package.
//!
//! Two sources feed a single map keyed by lowercased name: the `require` table of every
//! locked package, and the manifests holding this project's own requirements. Which
//! manifests those are is decided in `checker`, and at a workspace root it is every member
//! read one at a time, never the merged manifest. Merging keeps each constraint and drops
//! the member that declared it, which is the one thing this command exists to report.
//!
//! A chain therefore ends at `root (your project)` only in a standalone project. In a
//! workspace it ends at the member, `acme/logger`, because a workspace root declares no
//! requirements of its own.
//!
//! The queried name has to be in `phast.lock`. A package added to a manifest but not yet
//! installed is refused, rather than reported as required by whoever asked for it.

pub mod checker;
mod printer;

use crate::args::{DevArg, MemberArg, ProjectArg};
use crate::hints;
use crate::output::Printer;
use crate::package::PackageRequest;
use crate::project;
use crate::project::lock;

use checker::WhyChecker;
use printer::{ViewMode, WhyPrinter};

use phast_core::lock::phast::PhastLock;
use phast_workspace::Workspace;

use anyhow::{Context, Result, bail};
use clap::Args;

#[derive(Args)]
pub struct WhyArgs {
    /// The vendor/package name to look up, e.g. `psr/log`.
    pub package: String,
    /// Show the full reverse-dependency tree instead of direct requirers only.
    #[arg(long)]
    pub tree: bool,
    #[command(flatten)]
    pub dev: DevArg,
    #[command(flatten)]
    pub project: ProjectArg,
    #[command(flatten)]
    pub member: MemberArg,
}

#[rustfmt::skip]
pub async fn run(args: WhyArgs, printer: &Printer) -> Result<()> {
    let workdir   = project::workdir(args.project.dir())?;
    let workspace = Workspace::resolve(&workdir)?;
    let scope     = workspace.scope(&workdir, args.member.path())?;

    let lock_path = scope.root().join("phast.lock");
    if !lock_path.exists() {
        bail!(hints::phast_lock_not_found());
    }

    let lock = PhastLock::read(&lock_path)
        .context("failed to read phast.lock")?;

    let package = PackageRequest::parse(&args.package)?
        .name
        .to_lowercase();

    let Some(locked) = lock::find(&lock, &package) else {
        bail!(hints::package_not_in_lock(&package));
    };
    let version = locked.version.clone();

    let mode = if args.tree { ViewMode::Tree } else { ViewMode::Flat };
    let why  = WhyChecker::new(&scope, &lock, args.dev.include())?
        .check(&package);

    WhyPrinter::new(mode, printer)
        .print(&package, &version, &why);

    Ok(())
}
