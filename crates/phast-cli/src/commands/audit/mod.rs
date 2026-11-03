//! Auditing installed packages for security advisories.
//!
//! Always workspace-wide, and there is no `--member` to narrow it: a workspace shares one
//! lock and one installed tree, so a vulnerability anywhere in it reaches every member.
//!
//! A surviving advisory exits the process with status 1, so a CI step fails on a finding.
//! That exit bypasses the `Result` path entirely, so nothing downstream of the print runs.

mod advisory;
mod osv;
mod packagist;
mod printer;

use crate::args::{DevArg, ProjectArg};
use crate::hints;
use crate::output::OutputFormat;
use crate::output::Printer;
use crate::project;
use crate::project::lock;

use advisory::AdvisoryProvider;
use osv::OsvAdvisoryProvider;
use packagist::PackagistAdvisoryProvider;
use printer::AdvisoryPrinter;

use std::collections::HashSet;

use phast_core::lock::phast::PhastLock;
use phast_registry::http::client::HttpClient;
use phast_workspace::Workspace;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use clap::{Args, ValueEnum};

/// Which security advisory source to query.
#[derive(Clone, ValueEnum)]
pub enum AuditProvider {
    /// Packagist security advisories (`packagist.org/api/security-advisories`).
    Packagist,
    /// Open Source Vulnerabilities database (`api.osv.dev`).
    Osv,
}

#[derive(Args)]
pub struct AuditArgs {
    /// Advisory provider to query.
    #[arg(long, value_enum, default_value_t = AuditProvider::Packagist)]
    pub provider: AuditProvider,
    #[command(flatten)]
    pub dev: DevArg,
    /// Only report advisories published on or after this date. Undated ones are kept.
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub since: Option<NaiveDate>,
    /// Advisory IDs to suppress from the report, e.g. CVE-2023-24813 (repeatable).
    #[arg(long, value_name = "ID")]
    pub ignore: Vec<String>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
    #[command(flatten)]
    pub project: ProjectArg,
}

#[rustfmt::skip]
pub async fn run(args: AuditArgs, printer: &Printer) -> Result<()> {
    let workdir = project::workdir(args.project.dir())?;

    // Audit is always workspace-wide: the workspace shares one lock and one
    // installed tree, so a vulnerability anywhere in it affects every member.
    let root = Workspace::resolve(&workdir)?.root().to_path_buf();
    let lock_path = root.join("phast.lock");
    if !lock_path.exists() {
        bail!(hints::phast_lock_not_found());
    }

    let lock = PhastLock::read(&lock_path)
        .context("failed to read phast.lock")?;

    let packages = lock::collect(&lock, args.dev.include());
    if packages.is_empty() {
        printer.info("No packages installed.");
        return Ok(());
    }

    let client = HttpClient::new()?;
    let mut advisories = match args.provider {
        AuditProvider::Packagist => {
            PackagistAdvisoryProvider::new(client)
                .query(&packages)
                .await?
        },
        AuditProvider::Osv => {
            OsvAdvisoryProvider::new(client)
                .query(&packages)
                .await?
        },
    };

    if let Some(since) = args.since {
        advisories
            .retain(|a| a.reported_at.map(|d| d >= since)
            .unwrap_or(true));
    }

    if !args.ignore.is_empty() {
        let ignored: HashSet<String> = args.ignore
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect();
        advisories.retain(|a| !ignored.contains(&a.id.to_ascii_lowercase()));
    }

    advisories.sort_by_key(|a| (a.package.clone(), a.id.clone()));
    AdvisoryPrinter::new(args.format, printer)
        .print(&advisories)?;

    if !advisories.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}
