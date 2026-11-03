//! Running the `rules` catalog over every manifest a project owns.
//!
//! Parse and type errors are already gone by the time a rule runs: `Workspace::resolve`
//! rejects them on load, the same call `install` makes. What is left here is the checks that
//! need a whole manifest in view, which is why this needs no lock, no `vendor/` and no
//! reachable registry.
//!
//! A workspace is covered whole, the root and every member, because a workspace keeps its
//! dependencies in members and a root holds none of its own. Two things narrow that to a
//! single member: `--member`, and a working directory inside one. The second is silent, so
//! `phast validate` run from `packages/logger` reports that member and exits zero whatever
//! the sibling manifests hold.

mod printer;
mod rule;
mod rules;
mod validator;

use crate::args::{MemberArg, ProjectArg};
use crate::hints;
use crate::output::Printer;
use crate::project;

pub use rule::{Finding, ValidationContext, ValidationRule, Violation};
pub use rule::{Rule, Severity};
pub use rules::{
    DanglingRepository, DuplicateDependency, MissingPathSource, OrphanSource, SelfDependency,
};
pub use validator::Validator;

use printer::ValidationPrinter;

use std::path::Path;

use phast_workspace::{Scope, Workspace};

use anyhow::{Result, bail};
use clap::Args;

#[derive(Args)]
pub struct ValidateArgs {
    /// Treat warnings as errors (exit non-zero on any warning).
    #[arg(long)]
    pub strict: bool,
    #[command(flatten)]
    pub project: ProjectArg,
    #[command(flatten)]
    pub member: MemberArg,
}

#[rustfmt::skip]
pub async fn run(args: ValidateArgs, printer: &Printer) -> Result<()> {
    let workdir = project::workdir(args.project.dir())?;
    if !workdir.join("phast.toml").exists() {
        bail!(hints::phast_toml_not_found());
    }

    let workspace = Workspace::resolve(&workdir)?;
    let targets   = resolve_targets(&workdir, args.member.path(), &workspace)?;
    let examined  = targets.len();

    let rules: Vec<Box<dyn ValidationRule>> = vec![
        Box::new(DuplicateDependency),
        Box::new(SelfDependency),
        Box::new(DanglingRepository),
        Box::new(OrphanSource),
        Box::new(MissingPathSource),
    ];

    let mut output = Vec::new();
    for scope in &targets {
        let findings = Validator::new(scope)
            .with_rules(&rules)
            .validate();
        output.push((scope, findings));
    }

    // Reported per manifest, so a member is named once rather than on every line of its own,
    // and the order follows the project: the root first, then its members.
    let report = ValidationPrinter::new(printer);
    for (scope, findings) in &output {
        report.print(scope.member(), findings);
    }

    let (errors, warnings) = output
        .iter()
        .flat_map(|(_, findings)| findings)
        .fold((0, 0), |(errors, warnings), finding| match finding.severity {
            Severity::Error   => (errors + 1, warnings),
            Severity::Warning => (errors, warnings + 1),
        });

    if errors > 0 {
        bail!(hints::manifests_have_errors(errors));
    }

    if args.strict && warnings > 0 {
        bail!(hints::manifests_have_warnings(warnings));
    }

    printer.success(hints::manifests_are_valid(warnings, examined));

    Ok(())
}

/// Every manifest this run checks.
///
/// One when narrowed to a member. Otherwise, the whole project: the root, then every member,
/// because in a workspace all dependencies live in members and checking only the root
/// reports nothing. A standalone project has no members, so it stays one manifest.
#[rustfmt::skip]
fn resolve_targets<'a>(
    dir: &Path,
    member: Option<&str>,
    workspace: &'a Workspace,
) -> Result<Vec<Scope<'a>>> {
    let scope = workspace.scope(dir, member)?;
    if scope.member().is_some() {
        return Ok(vec![scope]);
    }

    // A member's own directory narrows to it, so each one arrives with its manifest expanded
    // and the member to attribute its findings to.
    let members = workspace
        .members()
        .iter()
        .map(|member| workspace.scope(member.path(), None));

    // The root as what it declares: its own repositories and sources, and no requirements.
    std::iter::once(workspace.root_scope()).chain(members).collect()
}
