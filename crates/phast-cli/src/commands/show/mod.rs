//! Reporting what `phast.lock` holds, as a list or one package at a time.
//!
//! Every field comes from `phast.lock` at `scope.root()` and none from `vendor/`, so this
//! reports what is locked rather than what is on disk: deleting `vendor/` changes nothing
//! about the output.
//!
//! `--member` and `--dev` narrow the listing only. Both are applied by retaining what
//! `Scope::includes` accepts, which a member answers with its direct requirements. Naming a
//! package takes the other branch, where `lock::find` searches the whole lock case
//! insensitively and consults no scope at all: `show psr/log --member packages/other` prints
//! psr/log in full, while `show --member packages/other` says no packages are installed.

mod printer;

use crate::args::{DevArg, MemberArg, ProjectArg};
use crate::hints;
use crate::output::{OutputFormat, Printer};
use crate::package::PackageRequest;
use crate::project;
use crate::project::lock;

use printer::ShowPrinter;

use phast_core::lock::phast::PhastLock;
use phast_workspace::Workspace;

use anyhow::{Context, Result, bail};
use clap::Args;

#[derive(Args)]
pub struct ShowArgs {
    /// One or more package names to inspect, e.g. `psr/log`.
    pub packages: Vec<String>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
    #[command(flatten)]
    pub project: ProjectArg,
    #[command(flatten)]
    pub member: MemberArg,
    #[command(flatten)]
    pub dev: DevArg,
}

#[rustfmt::skip]
pub async fn run(args: ShowArgs, printer: &Printer) -> Result<()> {
    let workdir   = project::workdir(args.project.dir())?;
    let workspace = Workspace::resolve(&workdir)?;
    let scope     = workspace.scope(&workdir, args.member.path())?;

    let lock_path = scope.root().join("phast.lock");
    if !lock_path.exists() {
        bail!(hints::phast_lock_not_found());
    }

    let mut lock = PhastLock::read(&lock_path)
        .context("failed to read phast.lock")?;

    let output = ShowPrinter::new(args.format, printer);

    if args.packages.is_empty() {
        if !args.dev.include() { lock.packages_dev.clear(); }

        lock.packages.retain(|p| {
            scope.includes(&p.name, args.dev.include())
        });
        lock.packages_dev.retain(|p| {
            scope.includes(&p.name, args.dev.include())
        });

        return output.print_all(&lock);
    }

    let requested = PackageRequest::parse_all(&args.packages)?;
    for (i, package) in requested.iter().enumerate() {
        let Some(package) = lock::find(&lock, &package.name) else {
            bail!(hints::package_not_in_lock(&package.name));
        };

        if i > 0 { printer.info(""); }
        output.print_one(&package)?;
    }

    Ok(())
}
