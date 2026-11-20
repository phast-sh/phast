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

use std::collections::BTreeMap;
use std::sync::Arc;

use phast_core::manifest::Script;
use phast_runtime::context::{Invocation, LoaderRules};
use phast_runtime::event::{LifecycleEvent, LifecycleEventManager};
use phast_runtime::executor::Environment;
use phast_runtime::runner::ScriptRunner;

/// Build a `LifecycleEventManager` wiring each event in `events` to the
/// script runner. Commands are responsible for checking `--no-scripts` and
/// passing `LifecycleEventManager::new()` directly when scripts are disabled.
#[rustfmt::skip]
pub fn register(
    scripts: BTreeMap<String, Script>,
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

    let mut lc = LifecycleEventManager::new();
    let runner = Arc::new(ScriptRunner::new(scripts, Environment::new()));

    for &event in events {
        let runner  = Arc::clone(&runner);
        let printer = printer.clone();
        lc.bind(event, move || {
            let invocation = Invocation {
                event: event.as_str().to_owned(),
                arguments: vec![],
                rules: LoaderRules::default(),
            };
            runner.run(&invocation, &printer)
        });
    }
    lc
}
