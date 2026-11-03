//! Reporting locked packages with newer releases available.
//!
//! Scoped like `phast why`: a single member when `--member` is given or the command runs
//! inside one, the merged workspace manifest at a workspace root, or the project manifest
//! standalone.
//!
//! Anything reported exits the process with status 1, so a CI step fails when a dependency
//! has fallen behind. That exit bypasses the `Result` path, so nothing after the print runs.

pub mod checker;
mod printer;

use crate::args::{DevArg, MemberArg, ProjectArg};
use crate::hints;
use crate::output::OutputFormat;
use crate::output::Printer;
use crate::project;
use crate::project::lock;

use checker::OutdatedChecker;
use printer::OutdatedPrinter;

use std::path::PathBuf;

use phast_core::lock::phast::PhastLock;
use phast_registry::http::auth::AuthConfig;
use phast_registry::http::client::HttpClient;
use phast_workspace::Workspace;

use anyhow::{Context, Result, bail};
use clap::Args;

#[derive(Args)]
pub struct OutdatedArgs {
    #[command(flatten)]
    pub dev: DevArg,
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
    #[command(flatten)]
    pub project: ProjectArg,
    #[command(flatten)]
    pub member: MemberArg,
}

#[rustfmt::skip]
pub async fn run(args: OutdatedArgs, cache_dir: PathBuf, printer: &Printer) -> Result<()> {
    let workdir   = project::workdir(args.project.dir())?;
    let workspace = Workspace::resolve(&workdir)?;
    let scope     = workspace.scope(&workdir, args.member.path())?;

    let lock_path = scope.root().join("phast.lock");
    if !lock_path.exists() {
        bail!(hints::phast_lock_not_found());
    }

    let lock = PhastLock::read(&lock_path)
        .context("failed to read phast.lock")?;

    // Member scope narrows the report to the member's direct requirements;
    // at a workspace root or standalone, every locked package is included.
    let mut packages = lock::collect(&lock, args.dev.include());
    packages.retain(|(name, _)| scope.includes(name, args.dev.include()));

    if packages.is_empty() {
        printer.info("No packages installed.");
        return Ok(());
    }

    let config = AuthConfig::load_from_manifest(scope.root(), &scope.manifest().config)?;
    let client = HttpClient::new()?
        .with_auth(config)
        .with_secure_http(scope.manifest().config.secure_http);

    let checker = OutdatedChecker::new(
        scope.root(),
        scope.manifest(),
        client,
        cache_dir,
        |w| printer.warning(w)
    ).await?;

    let mut output = checker
        .check(&packages)
        .await?;

    output.sort_by_key(|e| e.package.clone());
    OutdatedPrinter::new(args.format, printer)
        .print(&output)?;

    if !output.is_empty() {
        std::process::exit(1);
    }

    Ok(())
}
