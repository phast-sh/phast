//! Failure messages for workflow assertions.
//!
//! Centralised here so every way a step can fail reports it in the same wording and
//! tone. Each function covers one assertion in [`crate::workflow::Step`], and the
//! message is the whole diagnostic a reader gets: the step and workflow names are
//! added as context by the caller.

use crate::workflow::SNAPSHOTS;

use std::path::Path;

/// A step exited with a code other than the one its spec declares.
///
/// Both streams are included because an unexpected exit is usually explained by
/// output the assertions never got to look at. `actual` is `None` when the process
/// was killed by a signal and carries no code.
#[rustfmt::skip]
pub fn unexpected_status(
    expected: i32,
    actual: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> String {
    let actual = actual.map_or_else(|| "signal".to_string(), |code| code.to_string());

    format!(
        "expected exit code {expected}, got {actual}\n\
         --- stdout ---\n{stdout}\n\
         --- stderr ---\n{stderr}",
    )
}

/// A substring the step requires on stdout never appeared.
pub fn stdout_missing(needle: &str, stdout: &str) -> String {
    format!(
        "stdout does not contain {needle:?}\n\
         --- stdout ---\n{stdout}",
    )
}

/// A substring the step requires on stderr never appeared.
pub fn stderr_missing(needle: &str, stderr: &str) -> String {
    format!(
        "stderr does not contain {needle:?}\n\
         --- stderr ---\n{stderr}",
    )
}

/// A path listed under `creates` does not exist once the step has run.
pub fn not_created(path: &Path) -> String {
    format!("expected {} to exist after the step", path.display())
}

/// A path listed under `absent` exists once the step has run.
pub fn not_absent(path: &Path) -> String {
    format!("expected {} not to exist after the step", path.display())
}

/// A step declares `matches` but its workflow has no snapshots to compare against.
pub fn no_snapshots_dir() -> String {
    format!("workflow declares `matches` but has no {SNAPSHOTS}/ directory")
}

/// A snapshot has no counterpart in the workdir.
pub fn not_produced(path: &Path) -> String {
    format!("{} is in {SNAPSHOTS}/ but was not produced", path.display())
}

/// A snapshot and the file the step produced differ.
///
/// Both are printed in full rather than diffed: snapshots are small, and a byte-exact
/// comparison usually fails over whitespace that a diff would hide.
pub fn snapshot_mismatch(path: &Path, expected: &str, actual: &str) -> String {
    let path = path.display();

    format!(
        "{path} differs from {SNAPSHOTS}/\n\
         --- expected ---\n{expected}\n\
         --- actual ---\n{actual}",
    )
}
