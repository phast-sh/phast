//! Lifecycle event manager construction for CLI commands.
//!
//! The wiring is the same for every command, so a command declares which events it cares
//! about and nothing more.
//!
//! The `Executor` behind those events is built with no directory, which means a hook runs
//! from wherever phast was invoked rather than from the project it is acting on. Under
//! `phast install --project-dir <project>`, `post-install-cmd` runs in the invocation directory,
//! where `vendor/` is not.

use crate::output::Printer;

use std::collections::BTreeMap;
use std::sync::Arc;

use phast_core::manifest::Script;
use phast_runtime::event::{LifecycleEvent, LifecycleEventManager};
use phast_runtime::executor::Executor;
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
    let mut lc = LifecycleEventManager::new();
    let runner = Arc::new(ScriptRunner::new(scripts, Executor::new()));

    for &event in events {
        let runner  = Arc::clone(&runner);
        let printer = printer.clone();
        lc.bind(event, move || runner.run(event.as_str(), &printer));
    }
    lc
}
