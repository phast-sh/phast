//! User-facing error messages for the runtime.

use std::process::ExitStatus;

/// A script process ran and exited non-zero.
///
/// `ExitStatus` renders itself as `exit status: 3` or `signal: 9 (SIGKILL)`,
/// so it supplies its own noun and the surrounding sentence should not repeat one.
pub fn script_failed(status: ExitStatus) -> String {
    format!("script exited with status {status}")
}

/// A process the executor tried to spawn never started, so nothing ran at all.
///
/// Covers every spawn it attempts, the PHP interpreter as much as the shell.
pub fn shell_start_failed() -> &'static str {
    "failed to start shell"
}

/// A script calls `@name`, but no script has that name.
pub fn script_missing(name: &str) -> String {
    format!("script `{name}` is referenced but not defined in [scripts]")
}

/// Scripts that call each other in a cycle.
pub fn script_cycle(path: &str) -> String {
    format!("scripts call each other in a cycle: {path}")
}

/// The executor got a `@name` call. Only the runner can run those.
pub fn reference_not_expanded(name: &str) -> String {
    format!("`@{name}` can only be run through the script runner")
}
