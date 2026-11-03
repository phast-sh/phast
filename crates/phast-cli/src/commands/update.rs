//! Choosing which packages may move, then re-resolving.
//!
//! Naming none is the only place in the CLI where the resolver is handed no previous lock, so
//! nothing is sticky and every package is free to land on the newest version its constraints
//! admit. `install` and `lock` pass their lock through as preferences instead, which keeps a
//! locked version wherever it still fits.
//!
//! Naming packages puts the stickiness back, and harder than preferences: everything not
//! named is pinned to its locked version outright, so only what was asked for can move.
//!
//! `--member`, or a working directory inside one, supplies that list from a workspace member
//! for when none was named. The lock stays workspace-wide either way, so this chooses which
//! packages may move, not who is affected by them moving.

use crate::args::{DevArg, DryRunArg, MemberArg, ProjectArg, RegistryArg, ScriptsArg};
use crate::installer::{Installer, UpdateOptions, UpdateScope};
use crate::lifecycle;
use crate::output::Printer;
use crate::package::PackageRequest;
use crate::project;

use std::path::PathBuf;

use phast_registry::http::auth::AuthConfig;
use phast_registry::http::client::HttpClient;
use phast_runtime::event::{LifecycleEvent, LifecycleEventManager};
use phast_workspace::{Member, Workspace};

use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct UpdateArgs {
    /// Packages to update. If empty, update all.
    pub packages: Vec<String>,
    #[command(flatten)]
    pub dev: DevArg,
    #[command(flatten)]
    pub scripts: ScriptsArg,
    #[command(flatten)]
    pub dry: DryRunArg,
    #[command(flatten)]
    pub registry: RegistryArg,
    #[command(flatten)]
    pub project: ProjectArg,
    #[command(flatten)]
    pub member: MemberArg,
}

#[rustfmt::skip]
pub async fn run(args: UpdateArgs, cache_dir: PathBuf, printer: &Printer) -> Result<()> {
    let workdir   = project::workdir(args.project.dir())?;
    let workspace = Workspace::resolve(&workdir)?;

    let member = workspace.scope(&workdir, args.member.path())?.member();
    let scope  = resolve_scope(member, &args.packages, args.dev.include())?;

    let lifecycle = if args.scripts.run() {
        lifecycle::register(workspace.manifest().scripts.clone(), printer, &[
            LifecycleEvent::PreUpdateCmd,
            LifecycleEvent::PostUpdateCmd,
            LifecycleEvent::PreAutoloadDump,
            LifecycleEvent::PostAutoloadDump,
        ])
    } else {
        LifecycleEventManager::new()
    };

    let config = AuthConfig::load_from_manifest(workspace.root(), &workspace.manifest().config)?;
    let client = HttpClient::new()?
        .with_auth(config)
        .with_secure_http(workspace.manifest().config.secure_http);

    let options = UpdateOptions {
        no_dev:  args.dev.no_dev,
        dry_run: args.dry.dry_run,
        scope,
    };

    let mut installer = Installer::new(args.registry.url(), client, cache_dir, lifecycle, workspace, printer.clone());

    printer.success("Running update...");
    installer.update(options).await
}

/// Translate the packages a command names, and the member it was narrowed to, into an
/// [`UpdateScope`].
///
/// Named packages win, being the most specific answer there is. Naming none leaves "all"
/// to be defined, which a member defines as what it requires.
fn resolve_scope(
    member: Option<&Member>,
    packages: &[String],
    include_dev: bool,
) -> Result<UpdateScope> {
    if !packages.is_empty() {
        return Ok(UpdateScope::Packages(PackageRequest::names(packages)?));
    }

    let Some(member) = member else {
        return Ok(UpdateScope::All);
    };

    // The member's own manifest, not the merged one: the merge keeps every constraint but
    // drops the member that declared it, so it cannot say what this one requires.
    let manifest = member.to_manifest()?;
    let required = manifest.requirements(include_dev).cloned().collect();

    Ok(UpdateScope::Packages(required))
}
