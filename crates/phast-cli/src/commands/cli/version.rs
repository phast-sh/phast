//! Output formatting for `phast cli version`: the version and build-metadata block.
//!
//! Build-time values (epoch, commit, rustc, target) come from `build.rs`; the
//! release identity comes from [`crate::release`].

use crate::output::Printer;
use crate::release;
use crate::styles;

use std::fmt::{self, Display, Formatter};

use chrono::DateTime;

/// Renders the `phast cli version` block: version, release name, build metadata.
pub struct VersionPrinter<'a> {
    printer: &'a Printer,
}

impl<'a> VersionPrinter<'a> {
    pub fn new(printer: &'a Printer) -> Self {
        Self { printer }
    }

    /// Print the version block.
    pub fn print(&self) {
        self.printer.info(VersionOutput::new().to_string());
    }
}

/// The `phast cli version` block, with build metadata captured at construction so
/// [`Display`] is pure rendering, e.g.
///
/// ```text
/// phast 0.1.0 Ai-Petri
///   build:   2026-06-26 19:45 UTC
///   commit:  3d23ef2 (2026-06-25)
///   rustc:   1.86.0
///   target:  aarch64-apple-darwin
/// ```
///
/// Fields whose build-time value is `unknown` (e.g. commit in a non-git build)
/// are omitted. Colors are disabled automatically when stdout is not a terminal.
struct VersionOutput {
    /// The build time, formatted `YYYY-MM-DD HH:MM UTC`.
    build: String,
    /// `<sha> (<date>)`, or just `<sha>`; `None` outside a git checkout.
    commit: Option<String>,
    /// The compiler version; `None` if unknown.
    rustc: Option<String>,
    /// The target triple; `None` if unknown.
    target: Option<String>,
}

#[rustfmt::skip]
impl VersionOutput {
    /// Capture the build metadata `build.rs` baked in.
    fn new() -> Self {
        let epoch: i64 = env!("PHAST_BUILD_EPOCH")
            .parse()
            .unwrap_or(0);
        let build = DateTime::from_timestamp(epoch, 0)
            .unwrap_or_default()
            .format("%Y-%m-%d %H:%M UTC")
            .to_string();

        let commit = match (
            env!("PHAST_COMMIT"),
            env!("PHAST_COMMIT_DATE"),
        ) {
            ("unknown", _)   => None,
            (sha, "unknown") => Some(sha.to_owned()),
            (sha, date)      => Some(format!("{sha} ({date})")),
        };

        Self {
            build,
            commit,
            rustc:  Self::known(env!("PHAST_RUSTC")),
            target: Self::known(env!("PHAST_TARGET")),
        }
    }

    /// `None` for the `unknown` sentinel `build.rs` emits, otherwise the value.
    fn known(value: &str) -> Option<String> {
        (value != "unknown").then(|| value.to_owned())
    }

    /// Write a dimmed, aligned `  label:   value` line, with a leading newline so
    /// the block carries no trailing one (`Printer::info` adds the final newline).
    /// The label is padded before styling so the ANSI codes do not skew alignment.
    #[rustfmt::skip]
    fn field(&self, f: &mut Formatter<'_>, label: &str, value: impl Display) -> fmt::Result {
        let label = format!("{:<7}", format!("{label}:"));
        write!(f, "\n  {}  {value}", styles::DEFAULT_HINT.apply_to(label))
    }
}

impl Display for VersionOutput {
    #[rustfmt::skip]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}",
            styles::DEFAULT_KEY.apply_to("phast"),
            styles::DEFAULT_VALUE.apply_to(release::RELEASE_VERSION),
            styles::DEFAULT_VALUE.apply_to(release::RELEASE_NAME).bold(),
        )?;

        self.field(f, "build", &self.build)?;

        if let Some(commit) = &self.commit {
            self.field(f, "commit", commit)?;
        }
        if let Some(rustc) = &self.rustc {
            self.field(f, "rustc",  rustc)?;
        }
        if let Some(target) = &self.target {
            self.field(f, "target", target)?;
        }

        Ok(())
    }
}
