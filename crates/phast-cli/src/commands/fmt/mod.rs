//! Formatting a project's manifests in place.
//!
//! A workspace is covered whole, the root and every member. `--member` narrows to one,
//! and so does running it from inside one.

mod printer;

use crate::args::{MemberArg, ProjectArg};
use crate::hints;
use crate::output::Printer;
use crate::project;

use printer::FmtPrinter;

use std::path::{Path, PathBuf};

use phast_core::manifest::phast::PhastManifest;
use phast_workspace::Workspace;

use anyhow::{Context, Result, bail};
use clap::Args;

#[derive(Args)]
pub struct FmtArgs {
    /// Check formatting without writing. Exits 1 if a manifest would change.
    #[arg(long)]
    pub check: bool,
    #[command(flatten)]
    pub project: ProjectArg,
    #[command(flatten)]
    pub member: MemberArg,
}

#[rustfmt::skip]
pub async fn run(args: FmtArgs, printer: &Printer) -> Result<()> {
    let workdir = project::workdir(args.project.dir())?;
    if !workdir.join("phast.toml").exists() {
        bail!(hints::phast_toml_not_found());
    }

    let workspace = Workspace::resolve(&workdir)?;
    let targets   = resolve_targets(&workdir, args.member.path(), &workspace)?;
    let examined  = targets.len();

    let mut unformatted = 0;

    let report = FmtPrinter::new(printer);
    for (name, dir) in targets {
        let target_dir = dir.join("phast.toml");
        let phast_toml = name
            .as_deref()
            .unwrap_or("phast.toml");

        let content = std::fs::read_to_string(&target_dir)
            .with_context(|| format!("failed to read {phast_toml}"))?;
        let formatted = PhastManifest::format(&content)
            .with_context(|| format!("failed to format {phast_toml}"))?;
        if formatted == content {
            continue;
        }

        unformatted += 1;
        let changes = args.check.then_some((
            &*content,
            &*formatted,
        ));
        report.print(name.as_deref(), changes);

        if args.check {
            continue;
        }

        std::fs::write(&target_dir, &formatted)
            .with_context(|| format!("failed to write {phast_toml}"))?;
    }

    if unformatted == 0 {
        printer.success(hints::manifests_already_formatted(examined));
        return Ok(());
    }

    if args.check {
        bail!(hints::manifests_not_formatted(unformatted, examined));
    }

    printer.success(hints::manifests_formatted(unformatted, examined));

    Ok(())
}

/// Every directory this run formats the `phast.toml` of, each with the name it is reported
/// under.
///
/// One when narrowed to a member, otherwise the whole project: the root, then every member.
/// A standalone project has no members, so it stays the one file it was.
///
/// Members are named as `validate` names them, by `[package].name`. A workspace root is
/// reported as `root`. A standalone project's file is unnamed, being the only one there
/// is to report.
#[rustfmt::skip]
fn resolve_targets(
    dir: &Path,
    member: Option<&str>,
    workspace: &Workspace,
) -> Result<Vec<(Option<String>, PathBuf)>> {
    let scope = workspace.scope(dir, member)?;
    if let Some(member) = scope.member() {
        return Ok(vec![(Some(member.to_string()), scope.dir().to_path_buf())]);
    }

    if !workspace.is_active() {
        return Ok(vec![(None, scope.dir().to_path_buf())]);
    }

    // Each member by its own name, so a diff says which package it belongs to.
    let members = workspace
        .members()
        .iter()
        .map(|member| (Some(member.to_string()), member.path().to_path_buf()));

    let root = (Some("root".to_owned()), workspace.root()
        .to_path_buf());

    // The root as what it holds: its own workspace declarations, and no requirements.
    Ok(std::iter::once(root).chain(members).collect())
}
