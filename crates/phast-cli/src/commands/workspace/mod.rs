//! Reporting the workspace a directory sits in, and the members it declares.
//!
//! The one command that calls `Workspace::find` instead of `Workspace::resolve`. They differ
//! only over what a standalone project means: `resolve` manufactures a memberless workspace
//! from the manifest in hand, which suits `install` and everything else with work to do
//! either way. Here that would be a report about a workspace that does not exist, so `None`
//! becomes `no workspace found. Add a [workspace] section to phast.toml to create one.`
//!
//! A workspace that loads at all has at least one member, since `Workspace::load` rejects
//! both an empty `members` list and a pattern matching no `phast.toml`.

mod printer;

use crate::args::ProjectArg;
use crate::hints;
use crate::output::Printer;
use crate::project;

use printer::{ViewMode, WorkspacePrinter};

use phast_workspace::Workspace;

use anyhow::{Result, bail};
use clap::Args;

#[derive(Args)]
pub struct WorkspaceArgs {
    // TODO: let `phast workspace` add members, not only list them.
    #[command(flatten)]
    pub project: ProjectArg,
}

pub async fn run(args: WorkspaceArgs, printer: &Printer) -> Result<()> {
    let workdir = project::workdir(args.project.dir())?;
    let Some(workspace) = Workspace::find(&workdir)? else {
        bail!(hints::workspace_not_found());
    };

    let output = WorkspacePrinter::new(ViewMode::Flat, printer);
    output.print_root(workspace.root());
    output.print_members(&workspace, &workdir);

    Ok(())
}
