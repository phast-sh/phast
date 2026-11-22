//! Lifecycle event manager construction for CLI commands.
//!
//! The wiring is the same for every command. A command passes the events it cares about,
//! the project, and the loader.
//!
//! The `Environment` behind those events is built with no directory, which means a hook runs
//! from wherever Phast was invoked rather than from the project it is acting on. Under
//! `phast install --project-dir <project>`, `post-install-cmd` runs in the invocation directory,
//! where `vendor/` is not.

use crate::hints;
use crate::output::Printer;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use phast_autoload::builder::Builder;
use phast_core::lock::phast::PhastLock;
use phast_core::manifest::{Autoload, Manifest, Script};
use phast_core::package::PackageName;
use phast_runtime::context::{Invocation, LoaderRules, Project};
use phast_runtime::event::{LifecycleEvent, LifecycleEventManager};
use phast_runtime::executor::Environment;
use phast_runtime::runner::ScriptRunner;
use phast_version::RequireSpec;
use phast_workspace::{MemberAutoload, Workspace};

use anyhow::Result;
use serde_json::Value as JsonValue;

/// Bind each event in `events` that has a script to the script runner.
///
/// Commands check `--no-scripts` themselves, and pass `LifecycleEventManager::new()`
/// when scripts are off.
#[rustfmt::skip]
pub fn register(
    scripts: BTreeMap<String, Script>,
    project: Project,
    loader: Loader,
    printer: &Printer,
    events: &[LifecycleEvent],
) -> LifecycleEventManager {
    // TODO:
    //  Map scripts, like Symfony Flex's `auto-scripts`, are read but not run.
    //  Remove this warning once maps either run, or are reported when the
    //  manifest is read.
    for (name, script) in &scripts {
        if let Script::Map(commands) = script {
            let commands: Vec<String> = commands
                .keys()
                .map(|command| format!("'{command}'"))
                .collect();
            printer.warning(hints::map_script_skipped(name, &commands.join(", ")));
        }
    }

    let mut event_manager = LifecycleEventManager::new();
    let registered_events: Vec<LifecycleEvent> = events
        .iter()
        .copied()
        .filter(|event| scripts.contains_key(event.as_str()))
        .collect();
    // Only events with a script get a handler, so the others never read the lock.
    if registered_events.is_empty() {
        return event_manager;
    }

    let environment = Environment::new().with_project(project);
    // Each handler outlives this function, so it shares the one runner instead of borrowing it.
    let script_runner = Arc::new(ScriptRunner::new(scripts, environment));
    // The handlers share the loader too, and ask it for rules only when their event fires.
    let script_loader = Arc::new(loader);

    for event in registered_events {
        let runner = Arc::clone(&script_runner);
        let loader = Arc::clone(&script_loader);
        let output = printer.clone();

        event_manager.bind(event, move || {
            let invocation = Invocation {
                event: event.as_str().to_owned(),
                arguments: vec![],
                rules: loader.rules()?,
            };
            runner.run(&invocation, &output)
        });
    }
    event_manager
}

/// The project scripts run in: `manifest`'s data, the vendor and bin directories
/// under `root`, and whether dev packages are part of the command.
pub fn project(manifest: &Manifest, root: &Path, dev_mode: bool) -> Project {
    let vendor_dir = root.join("vendor");

    Project {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        extra: manifest.extra.clone(),
        require: requirements(&manifest.require),
        require_dev: requirements(&manifest.require_dev),
        autoload: autoload(&manifest.autoload),
        bin_dir: vendor_dir.join("bin"),
        vendor_dir,
        dev_mode,
    }
}

/// Each requirement as its package name and constraint, sorted by name.
fn requirements(requirements: &HashMap<PackageName, RequireSpec>) -> BTreeMap<String, String> {
    requirements
        .iter()
        .map(|(name, spec)| (name.as_str().to_owned(), spec.to_string()))
        .collect()
}

/// The autoload rules with Composer's key names, as a script's `getAutoload()` returns them.
///
/// TODO: building this by hand is a stopgap, needs to be revisited.
fn autoload(autoload: &Autoload) -> JsonValue {
    serde_json::json!({
        "psr-4": autoload.psr4,
        "psr-0": autoload.psr0,
        "classmap": autoload.classmap,
        "files": autoload.files,
        "exclude-from-classmap": autoload.exclude,
    })
}

/// The class loader a PHP callback starts with, before the real autoloader exists.
///
/// `new` reads the members' rules once. `rules` reads the lock every time, because
/// `install` and `update` write it while the command runs.
///
/// `new` also makes the root and each member's directory absolute. Every rule's
/// directory starts from one of them, so the rules are absolute too. PHP may run
/// in another directory, where a relative path would point somewhere else.
pub struct Loader {
    /// The workspace root, where `phast.lock` and `vendor/` live. Absolute.
    root: PathBuf,
    /// Each member's autoload rules. Their paths are relative to the member's directory,
    /// which is absolute.
    members: Vec<MemberAutoload>,
    /// Whether dev packages and dev rules are included.
    dev_mode: bool,
}

impl Loader {
    /// A loader for `workspace`'s packages and members.
    ///
    /// Fails when a member's manifest doesn't convert.
    pub fn new(workspace: &Workspace, dev_mode: bool) -> Result<Self> {
        let mut members = workspace
            .members()
            .iter()
            .map(|member| member.autoloads())
            .collect::<Result<Vec<_>>>()?;
        for member in &mut members {
            member.dir = std::path::absolute(&member.dir)?;
        }

        Ok(Self {
            root: std::path::absolute(workspace.root())?,
            members,
            dev_mode,
        })
    }

    /// The PSR-4 and PSR-0 rules from the lock as it is now.
    ///
    /// Without a lock there are none, as before the first install.
    #[rustfmt::skip]
    pub fn rules(&self) -> Result<LoaderRules> {
        let lock_path = self.root.join("phast.lock");
        if !lock_path.exists() {
            return Ok(LoaderRules::default());
        }

        let vendor_dir = self.root.join("vendor");

        let lock  = PhastLock::read(&lock_path)?;
        let rules = Builder::new(
            &vendor_dir,
            self.dev_mode,
        ).rules(
            &lock,
            &self.members,
        )?;

        Ok(LoaderRules {
            psr4: rules.psr4,
            psr0: rules.psr0,
        })
    }
}
