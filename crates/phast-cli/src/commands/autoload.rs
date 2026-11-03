//! Rebuilding the autoloader from a lock that is already written.
//!
//! Three trees are rewritten, not one: `vendor/autoload.php`, everything under
//! `vendor/phast/`, and `vendor/composer/installed.json`, which is the metadata
//! Composer-era tooling reads. `vendor/bin/` is relinked afterward, outside the dump hooks,
//! so a `post-autoload-dump` script runs before the stubs it may expect are in place.
//!
//! Classmap-authoritative mode is the flag ORed with `[config]`, so
//! `--classmap-authoritative` turns it on for one run and nothing here turns it off again
//! once the manifest asks for it.
//!
//! There is no `--member`. One lock and one `vendor/` serve the whole project, so this reads
//! and writes at `Workspace::root()` whichever member the working directory sits in.

use crate::args::{ClassmapArg, DevArg, ProjectArg, ScriptsArg};
use crate::installer::Autoloader;
use crate::lifecycle;
use crate::output::Printer;
use crate::project;

use phast_autoload::AutoloadOptions;
use phast_core::lock::phast::PhastLock;
use phast_runtime::event::{LifecycleEvent, LifecycleEventManager};
use phast_workspace::Workspace;

use anyhow::{Context, Result};
use clap::Args;

#[derive(Args)]
pub struct AutoloadArgs {
    #[command(flatten)]
    pub dev: DevArg,
    #[command(flatten)]
    pub scripts: ScriptsArg,
    #[command(flatten)]
    pub classmap: ClassmapArg,
    #[command(flatten)]
    pub project: ProjectArg,
}

#[rustfmt::skip]
pub async fn run(args: AutoloadArgs, printer: &Printer) -> Result<()> {
    let workdir   = project::workdir(args.project.dir())?;
    let workspace = Workspace::resolve(&workdir)?;

    let lock_path  = workspace.root().join("phast.lock");
    let vendor_dir = workspace.root().join("vendor");

    let lock = PhastLock::read(&lock_path)
        .context("could not read phast.lock; run `phast install` first")?;

    let lifecycle = if args.scripts.run() {
        lifecycle::register(workspace.manifest().scripts.clone(), printer,&[
            LifecycleEvent::PreAutoloadDump,
            LifecycleEvent::PostAutoloadDump,
        ])
    } else {
        LifecycleEventManager::new()
    };

    let classmap_authoritative = args.classmap.classmap_authoritative
                || workspace.manifest().config.classmap_authoritative;

    let options = AutoloadOptions {
        include_dev: args.dev.include(),
        authoritative: classmap_authoritative,
    };

    Autoloader::new(vendor_dir, lifecycle, printer.clone())
        .generate(&lock, options, &workspace)
}
