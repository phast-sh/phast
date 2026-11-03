//! Writing `phast.lock` and stopping there.
//!
//! `--refresh` re-resolves a lock the freshness check would have accepted, and that is all it
//! does: the existing lock still reaches the solver as preferences, so a locked version the
//! constraints still admit is chosen again. It rewrites the file without moving anything, and
//! is not a quiet spelling of `update`.
//!
//! No lifecycle event is registered at all, so `pre-install-cmd` and the autoload-dump hooks
//! stay silent here even though `install` fires them for the same resolution. There is no
//! `--no-scripts` to explain the absence, because there is nothing to switch off.
//!
//! There is no `--member` either. One lock covers the whole project, so there is no smaller
//! unit to write.
//!
//! Nor is there a `--no-dev`. The lock records the dev packages whatever gets installed, and
//! `lock` installs nothing, so the flag would have nothing left to change.

use crate::args::{ProjectArg, RegistryArg};
use crate::installer::{Installer, LockOptions};
use crate::output::Printer;
use crate::project;

use std::path::PathBuf;

use phast_registry::http::auth::AuthConfig;
use phast_registry::http::client::HttpClient;
use phast_runtime::event::LifecycleEventManager;
use phast_workspace::Workspace;

use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct LockArgs {
    /// Update the lock file even if it is already fresh.
    #[arg(long)]
    pub refresh: bool,
    #[command(flatten)]
    pub registry: RegistryArg,
    #[command(flatten)]
    pub project: ProjectArg,
}

#[rustfmt::skip]
pub async fn run(args: LockArgs, cache_dir: PathBuf, printer: &Printer) -> Result<()> {
    let workdir   = project::workdir(args.project.dir())?;
    let workspace = Workspace::resolve(&workdir)?;

    let config = AuthConfig::load_from_manifest(workspace.root(), &workspace.manifest().config)?;
    let client = HttpClient::new()?
        .with_auth(config)
        .with_secure_http(workspace.manifest().config.secure_http);

    let options = LockOptions {
        refresh: args.refresh,
    };

    let event_manager = LifecycleEventManager::new();
    let mut installer = Installer::new(args.registry.url(), client, cache_dir, event_manager, workspace, printer.clone());

    printer.success("Running lock...");
    installer.lock(options).await
}
