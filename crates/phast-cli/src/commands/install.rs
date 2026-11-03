//! Turning a lock into a populated `vendor/`.
//!
//! A lock matching the manifest is used as it stands and nothing is re-resolved. A stale one
//! is not discarded either: it reaches the resolver as preferences, so any locked version the
//! manifest still admits is kept and the diff stays as small as the edit that caused it. That
//! is what separates this from `update`, which hands the resolver no lock at all and lets
//! every package move to the newest version allowed.
//!
//! `--member` narrows what reaches `vendor/`, not what gets locked. The plan becomes that
//! member's dependency closure, so installing `packages/other` never fetches `psr/log` even
//! though the workspace-wide lock still lists it. The member has to declare a `[package].name`,
//! because the closure is resolved on that identity.

use crate::args::{ClassmapArg, DevArg, DryRunArg, MemberArg, ProjectArg, RegistryArg, ScriptsArg};
use crate::hints;
use crate::installer::{InstallOptions, InstallScope, Installer};
use crate::lifecycle;
use crate::output::Printer;
use crate::project;

use std::path::PathBuf;

use phast_registry::http::auth::AuthConfig;
use phast_registry::http::client::HttpClient;
use phast_runtime::event::{LifecycleEvent, LifecycleEventManager};
use phast_workspace::{Member, Workspace};

use anyhow::{Result, anyhow};
use clap::Args;

#[derive(Args)]
pub struct InstallArgs {
    #[command(flatten)]
    pub dev: DevArg,
    #[command(flatten)]
    pub scripts: ScriptsArg,
    #[command(flatten)]
    pub classmap: ClassmapArg,
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
pub async fn run(args: InstallArgs, cache_dir: PathBuf, printer: &Printer) -> Result<()> {
    let workdir   = project::workdir(args.project.dir())?;
    let workspace = Workspace::resolve(&workdir)?;

    let member = workspace.scope(&workdir, args.member.path())?.member();
    let scope  = resolve_scope(member)?;

    let lifecycle = if args.scripts.run() {
        lifecycle::register(workspace.manifest().scripts.clone(), printer, &[
            LifecycleEvent::PreInstallCmd,
            LifecycleEvent::PostInstallCmd,
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

    let options = InstallOptions {
        no_dev:  args.dev.no_dev,
        dry_run: args.dry.dry_run,
        authoritative: args.classmap.classmap_authoritative,
        scope,
    };

    let mut installer = Installer::new(args.registry.url(), client, cache_dir, lifecycle, workspace, printer.clone());

    printer.success("Running install...");
    installer.install(options).await
}

/// Translate the member a command was narrowed to into an [`InstallScope`].
///
/// `None` is a full install. A member must declare a `[package].name`,
/// the identity its lock closure resolves on.
fn resolve_scope(member: Option<&Member>) -> Result<InstallScope> {
    let Some(member) = member else {
        return Ok(InstallScope::Full);
    };

    // Beyond existing, which is all the flag itself promises: a closure resolves on the
    // member's package identity, so one without a name has nothing to scope to.
    let name = member
        .name()
        .ok_or_else(|| anyhow!("{}", hints::member_without_name(&member.to_string())))?;

    Ok(InstallScope::Member(name.to_owned()))
}
