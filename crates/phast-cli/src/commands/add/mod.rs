//! Adding dependencies to a manifest.
//!
//! A version is resolved against a registry only when the request leaves one open. An
//! explicit `psr/log:^3.0`, or a package the workspace root already pins, is recorded
//! without reaching a registry at all; `--registry-url` chooses who answers when one is
//! needed.
//!
//! The manifest is written from inside the installer's callback, once versions are known.
//! The callback reports whether it wrote anything, so a request the manifest already
//! satisfies stops there rather than running an install nobody needs.

mod plan;
mod printer;

use crate::args::{MemberArg, NoInstallArg, ProjectArg, RegistryArg};
use crate::hints;
use crate::installer::{AddOptions, InstallOptions, Installer};
use crate::output::Printer;
use crate::package::PackageRequest;
use crate::project;
use crate::project::manifest::{Change, Editor, Section};

use printer::AddPrinter;

use std::collections::HashMap;
use std::path::PathBuf;

use phast_core::PackageName;
use phast_registry::http::auth::AuthConfig;
use phast_registry::http::client::HttpClient;
use phast_runtime::event::LifecycleEventManager;
use phast_version::Version;
use phast_workspace::Workspace;

use anyhow::{Result, bail};
use clap::Args;

#[derive(Args)]
pub struct AddArgs {
    /// One or more packages, e.g. `psr/log:^x.y`.
    pub packages: Vec<String>,
    /// Add to require-dev instead of require.
    #[arg(long)]
    pub dev: bool,
    #[command(flatten)]
    pub install: NoInstallArg,
    #[command(flatten)]
    pub registry: RegistryArg,
    #[command(flatten)]
    pub project: ProjectArg,
    #[command(flatten)]
    pub member: MemberArg,
}

#[rustfmt::skip]
pub async fn run(args: AddArgs, cache_dir: PathBuf, printer: &Printer) -> Result<()> {
    if args.packages.is_empty() {
        bail!(hints::no_packages_to_add());
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

    // What the workspace root already pins for the packages being added. A pinned one needs
    // no version chosen and is recorded as { workspace = true }, so the member never spells
    // a constraint the root owns.
    let known = plan::known(&requests, &workspace)?;

    // What the solver has to choose from, or `None` when every constraint is already known
    // and there is nothing to solve. That absence is what keeps an `add` with explicit
    // constraints, or one the root pins, from reaching a registry at all.
    let requirements = plan::requirements(&requests, &known, args.dev)?;

    // Runs once the versions are known, which is the first point anything may be written.
    // Reports whether it wrote: `false` says the manifest already held what was asked for,
    // and stops an install nobody needs.
    let record = |selected: &HashMap<PackageName, Version>| {
        let section = if args.dev { Section::RequireDev } else { Section::Require };
        // TODO: plan::changes takes five parameters;
        //       group the per-invocation ones into a struct.
        let changes = plan::changes(&requests, &manifest, section, selected, &known)?;
        AddPrinter::new(section, printer).print(&changes);

        if changes.iter().all(Change::is_noop) {
            printer.info("Nothing to change.");
            return Ok(false);
        }

        manifest.apply(section, &changes);
        manifest.save()?;
        printer.success("phast.toml updated.");

        Ok(true)
    };

    let config = AuthConfig::load_from_manifest(workspace.root(), &workspace.manifest().config)?;
    let client = HttpClient::new()?
        .with_auth(config)
        .with_secure_http(workspace.manifest().config.secure_http);

    let event_manager = LifecycleEventManager::new();
    let mut installer = Installer::new(args.registry.url(), client, cache_dir, event_manager, workspace, printer.clone());

    let recorded = installer
        .add(AddOptions { requirements }, record)
        .await?;
    if !recorded || args.install.skip() {
        return Ok(());
    }

    printer.success("Running install...");
    installer.install(InstallOptions::default()).await
}
