//! Removing dependencies from a manifest.
//!
//! A request is refused unless the section already declares it, so anything that gets past
//! planning changes the manifest. That is why there is no "nothing to change" path here,
//! unlike `add`, which can find the manifest already satisfies what was asked and skip the
//! install that follows.

mod plan;
mod printer;

use crate::args::{MemberArg, NoInstallArg, ProjectArg};
use crate::hints;
use crate::installer::{InstallOptions, Installer};
use crate::output::Printer;
use crate::package::PackageRequest;
use crate::project;
use crate::project::manifest::{Editor, Section};

use printer::RemovePrinter;

use std::path::PathBuf;

use phast_registry::http::auth::AuthConfig;
use phast_registry::http::client::HttpClient;
use phast_runtime::event::LifecycleEventManager;
use phast_workspace::Workspace;

use anyhow::{Result, bail};
use clap::Args;

#[derive(Args)]
pub struct RemoveArgs {
    /// One or more packages to remove, e.g. `psr/log`.
    pub packages: Vec<String>,
    /// Remove from require-dev instead of require.
    #[arg(long)]
    pub dev: bool,
    #[command(flatten)]
    pub install: NoInstallArg,
    #[command(flatten)]
    pub project: ProjectArg,
    #[command(flatten)]
    pub member: MemberArg,
}

#[rustfmt::skip]
pub async fn run(args: RemoveArgs, cache_dir: PathBuf, printer: &Printer) -> Result<()> {
    if args.packages.is_empty() {
        bail!(hints::no_packages_to_remove());
    }

    let workdir = project::workdir(args.project.dir())?;
    if !workdir.join("phast.toml").exists() {
        bail!(hints::phast_toml_not_found());
    }

    // Resolved after that check: locating a member walks up to the workspace root,
    // which needs a manifest to find, and would report its own absence first.
    let workspace  = Workspace::resolve(&workdir)?;
    let target_dir = workspace.scope(&workdir, args.member.path())?.dir().to_path_buf();

    let mut manifest = Editor::open(&target_dir)?;
    if manifest.is_workspace_root() {
        bail!(hints::member_required_for_dependencies());
    }

    let requests = PackageRequest::parse_all(&args.packages)?;
    let section  = if args.dev { Section::RequireDev } else { Section::Require };

    let changes = plan::changes(&requests, &manifest, section)?;
    RemovePrinter::new(section, printer).print(&changes);

    manifest.apply(section, &changes);
    manifest.save()?;
    printer.success("phast.toml updated.");

    if args.install.skip() {
        return Ok(());
    }

    // Re-read what was just saved: the install hashes the manifest to tell whether the
    // lock is fresh, and the one resolved above still declares the removed packages.
    let workspace = Workspace::resolve(workspace.root())?;

    let config = AuthConfig::load_from_manifest(workspace.root(), &workspace.manifest().config)?;
    let client = HttpClient::new()?
        .with_auth(config)
        .with_secure_http(workspace.manifest().config.secure_http);

    let event_manager = LifecycleEventManager::new();
    let mut installer = Installer::new(None, client, cache_dir, event_manager, workspace, printer.clone());

    printer.success("Running install...");
    installer.install(InstallOptions::default()).await
}
