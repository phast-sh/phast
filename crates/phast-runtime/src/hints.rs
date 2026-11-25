//! User-facing error messages for the runtime.

use std::process::ExitStatus;

/// A process the executor tried to spawn never started, so nothing ran at all.
///
/// Covers every spawn it attempts, the PHP interpreter as much as the shell.
pub fn shell_start_failed() -> &'static str {
    "failed to start shell"
}

/// A script entry ran and exited non-zero, or was stopped by a signal.
#[rustfmt::skip]
pub fn script_failed(entry: &str, event: &str, status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("script {entry} ({event}) failed with exit code {code}"),
        None       => format!("script {entry} ({event}) was stopped ({status})"),
    }
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
