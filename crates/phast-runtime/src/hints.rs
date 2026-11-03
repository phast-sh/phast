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
