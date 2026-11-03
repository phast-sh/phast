//! Terminal output utilities.
//!
//! All printing goes through [`Printer`]. Nothing else in phast writes to stdout or stderr
//! directly, apart from `init`'s prompt and `fmt`'s diff, which own their own newlines. This
//! ensures quiet mode and verbosity flags are respected globally.

use crate::styles;

use std::time::Duration;

use phast_runtime::executor::ScriptOutput;

use clap::ValueEnum;
use console::Term;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// Output format shared by commands that support both human-readable and machine-readable output.
#[derive(Clone, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable columnar output with color.
    Text,
    /// Machine-readable JSON.
    Json,
}

/// Output verbosity level, derived from CLI flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verbosity {
    /// Suppress all output except errors (`-q` / `--quiet`).
    Quiet,
    /// Default: informational messages and progress indicators.
    Normal,
    /// Show additional diagnostic messages (`-v`).
    Verbose,
    /// Show even more detail (`-vv`).
    VeryVerbose,
}

/// Central output handler for all CLI commands.
///
/// Every method here consults `verbosity` before it writes, which is why a `Printer` is
/// threaded down into each phase rather than each phase reading the flags for itself.
#[derive(Clone)]
pub struct Printer {
    /// Controls which messages are shown based on CLI flags.
    verbosity: Verbosity,
    /// Shared multi-progress container that keeps spinners and bars aligned.
    multi: MultiProgress,
}

impl Printer {
    /// Create a new `Printer` with the given verbosity level.
    pub fn new(verbosity: Verbosity) -> Self {
        Self {
            verbosity,
            multi: MultiProgress::new(),
        }
    }

    /// Print a plain informational message. Suppressed in quiet mode.
    pub fn info(&self, msg: impl AsRef<str>) {
        if self.verbosity >= Verbosity::Normal {
            println!("{}", msg.as_ref());
        }
    }

    /// Print a success message in green. Suppressed in quiet mode.
    pub fn success(&self, msg: impl AsRef<str>) {
        if self.verbosity >= Verbosity::Normal {
            println!("{}", styles::VERBOSITY_SUCCESS.apply_to(msg.as_ref()));
        }
    }

    /// Print a warning message in yellow to stderr. Suppressed in quiet mode.
    pub fn warning(&self, msg: impl AsRef<str>) {
        if self.verbosity >= Verbosity::Normal {
            eprintln!("{}", styles::VERBOSITY_WARNING.apply_to(msg.as_ref()));
        }
    }

    /// Print an error message in red bold to stderr. Always shown regardless of verbosity.
    pub fn error(&self, msg: impl AsRef<str>) {
        eprintln!("{}", styles::VERBOSITY_ERROR.apply_to(msg.as_ref()));
    }

    /// Print a dim/muted message. Suppressed in quiet mode.
    pub fn hint(&self, msg: impl AsRef<str>) {
        if self.verbosity >= Verbosity::Normal {
            println!("{}", styles::VERBOSITY_HINT.apply_to(msg.as_ref()));
        }
    }

    /// Print a per-phase summary ("Locked 289 packages in 6.53s"). Only shown at `-v` or higher.
    pub fn phase(&self, verb: &str, total: usize, elapsed: f64) {
        if self.verbosity >= Verbosity::Verbose {
            self.hint(format!("{verb} {total} packages in {elapsed:.2}s"));
        }
    }

    /// Print the total completion line ("Finished in 5.90s (289 packages)"). Shown in normal mode.
    pub fn summary(&self, total: usize, elapsed: f64) {
        if self.verbosity >= Verbosity::Normal {
            self.success(format!("Finished in {elapsed:.2}s ({total} packages)"));
        }
    }

    /// Add an indeterminate spinner to the multi-progress display.
    ///
    /// Returns a `Spinner` guard that hides the terminal cursor for the duration.
    /// The cursor is restored automatically when the guard is dropped, so it is
    /// safe even if the caller exits early via `?`.
    pub fn spinner(&self, message: &str) -> Spinner {
        if self.verbosity == Verbosity::Quiet {
            return Spinner {
                progress_bar: ProgressBar::hidden(),
                verbosity: self.verbosity,
            };
        }

        let progress_bar = self.multi.add(ProgressBar::new_spinner());
        progress_bar.set_style(
            ProgressStyle::with_template("  {spinner:.green} {msg}")
                .expect("hardcoded template is always valid")
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "),
        );
        progress_bar.set_message(message.to_string());
        progress_bar.enable_steady_tick(Duration::from_millis(50));

        let _ = Term::stderr().hide_cursor();

        Spinner {
            progress_bar,
            verbosity: self.verbosity,
        }
    }
}

impl ScriptOutput for Printer {
    /// Print the script command before it runs,
    /// so the user can see what is being executed.
    fn info(&self, msg: &str) {
        self.info(msg);
    }
}

/// Guard returned by `Printer::spinner`.
///
/// Hides the terminal cursor while alive and restores it on drop.
pub struct Spinner {
    /// Underlying indicatif progress bar used for rendering.
    progress_bar: ProgressBar,
    /// Verbosity level inherited from the `Printer` that created this spinner.
    verbosity: Verbosity,
}

impl Spinner {
    /// Update the spinner for an in-progress package.
    ///
    /// Always updates the spinner message. At `-vv`, also prints a dedicated
    /// line above the spinner so each package appears in the scrollback.
    pub fn tick(&self, verb: &str, name: &str, version: &str) {
        if self.verbosity >= Verbosity::VeryVerbose {
            self.progress_bar.println(format!(
                "  - {verb} {} ({})",
                styles::PACKAGE_NAME.apply_to(name),
                styles::PACKAGE_VERSION.apply_to(version),
            ));
        }

        self.progress_bar
            .set_message(format!("{verb} {name} ({version})"));
    }

    /// Print a line above the spinner without disturbing it.
    pub fn println(&self, msg: impl AsRef<str>) {
        self.progress_bar.println(msg);
    }

    /// Stop the spinner and clear it from the terminal.
    ///
    /// The cursor is restored via the `Drop` impl, so it is safe to call
    /// this or let the guard drop naturally, either way the cursor comes back.
    pub fn finish_and_clear(self) {
        self.progress_bar.finish_and_clear();
    }
}

impl Drop for Spinner {
    /// Restore the terminal cursor when the spinner is dropped.
    ///
    /// Called automatically on both normal completion and early exit via `?`,
    /// so the cursor is never left hidden even if a command returns an error.
    fn drop(&mut self) {
        if self.verbosity == Verbosity::Quiet {
            return;
        }

        let _ = Term::stderr().show_cursor();
    }
}
