//! Lifecycle event manager construction for CLI commands.
//!
//! The wiring is the same for every command, so a command declares which events it cares
//! about and nothing more.
//!
//! The `Environment` behind those events is built with no directory, which means a hook runs
//! from wherever Phast was invoked rather than from the project it is acting on. Under
//! `phast install --project-dir <project>`, `post-install-cmd` runs in the invocation directory,
//! where `vendor/` is not.

use crate::hints;
use crate::output::Printer;

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use phast_core::manifest::{Autoload, Manifest, Script};
use phast_core::package::PackageName;
use phast_runtime::context::{Invocation, LoaderRules, Project};
use phast_runtime::event::{LifecycleEvent, LifecycleEventManager};
use phast_runtime::executor::Environment;
use phast_runtime::runner::ScriptRunner;
use phast_version::RequireSpec;

use serde_json::Value as JsonValue;

/// Build a `LifecycleEventManager` wiring each event in `events` to the
/// script runner. Commands are responsible for checking `--no-scripts` and
/// passing `LifecycleEventManager::new()` directly when scripts are disabled.
#[rustfmt::skip]
pub fn register(
    scripts: BTreeMap<String, Script>,
    project: Project,
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

    let mut manager = LifecycleEventManager::new();
    let environment = Environment::new().with_project(project);

    // One runner serves every event. Each handler outlives this function, so it holds
    // its own pointer to the runner, rather than a borrow or a copy of the scripts.
    let script_runner = Arc::new(
        ScriptRunner::new(
            scripts,
            environment,
        ),
    );

    for &event in events {
        let runner = Arc::clone(&script_runner);
        let output = printer.clone();
        manager.bind(event, move || {
            let invocation = Invocation {
                event: event.as_str().to_owned(),
                arguments: vec![],
                rules: LoaderRules::default(),
            };
            runner.run(&invocation, &output)
        });
    }
    manager
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
