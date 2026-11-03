//! Running a script from a manifest's `[scripts]` section.
//!
//! `Workspace::scope` decides both which manifest is read and where the commands run.
//! Narrowed to a member, by `--member` or by a working directory inside one, it answers with
//! that member's `[scripts]`; unnarrowed it answers with the root's, which the merged
//! manifest carries through untouched. The executor is handed `scope.dir()`, so a script
//! narrowed to `packages/logger` runs from that directory while `phast.lock` and `vendor/`
//! stay at the workspace root, and a relative `vendor/bin/phpunit` will not resolve from
//! there.
//!
//! The name is checked against the section before the runner is built, because
//! `ScriptRunner::run_named` returns `Ok(())` for a name it does not hold. Without the
//! check, a misspelled script would exit zero having done nothing.

mod printer;

use crate::args::{MemberArg, ProjectArg};
use crate::hints;
use crate::output::Printer;
use crate::project;

use printer::RunPrinter;

use phast_runtime::executor::Executor;
use phast_runtime::runner::ScriptRunner;
use phast_workspace::Workspace;

use anyhow::{Result, bail};
use clap::Args;

#[derive(Args)]
pub struct RunArgs {
    /// Name of the script to run, as defined in the scripts table of phast.toml.
    pub script: Option<String>,
    #[command(flatten)]
    pub project: ProjectArg,
    #[command(flatten)]
    pub member: MemberArg,
    /// Extra arguments appended to the last command in the script array.
    #[arg(last = true)]
    pub extra_args: Vec<String>,
}

#[rustfmt::skip]
pub async fn run(args: RunArgs, printer: &Printer) -> Result<()> {
    let workdir   = project::workdir(args.project.dir())?;
    let workspace = Workspace::resolve(&workdir)?;

    let scope   = workspace.scope(&workdir, args.member.path())?;
    let scripts = &scope.manifest().scripts;

    let Some(script) = args.script else {
        RunPrinter::new(printer).print(scripts);
        return Ok(());
    };

    if !scripts.contains_key(&script) {
        bail!(hints::script_not_found(&script));
    }

    let executor = Executor::new()
        .with_dir(scope.dir());

    let runner = ScriptRunner::new(scripts.clone(), executor);
    runner.run_named(&script, &args.extra_args, printer)?;

    Ok(())
}
