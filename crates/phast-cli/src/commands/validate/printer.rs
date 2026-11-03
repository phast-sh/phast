//! Rendering validation findings to the console.

use super::rule::{Finding, Severity, Violation};

use crate::output::Printer;

use std::fmt::{Display, Formatter, Result as FmtResult};

use phast_workspace::Member;

/// Prints validation findings, colored by severity.
pub struct ValidationPrinter<'a> {
    printer: &'a Printer,
}

impl<'a> ValidationPrinter<'a> {
    pub fn new(printer: &'a Printer) -> Self {
        Self { printer }
    }

    /// Print each finding as one message, colored by severity.
    ///
    /// Both severities go to stderr, so redirecting stdout still leaves every finding
    /// visible. What severity changes is whether the message survives `--quiet`: an error
    /// always prints, a warning does not. Counting happens elsewhere and is not suppressed,
    /// so `validate --strict --quiet` can fail with `2 warning(s) found.` and nothing on
    /// screen saying which two.
    ///
    /// `member` names the workspace member the findings came from, which a project holding
    /// several manifests needs and a project holding one does not: unnamed output is what a
    /// standalone project has always printed.
    #[rustfmt::skip]
    pub fn print(&self, member: Option<&Member>, findings: &[Finding]) {
        for finding in findings {
            let diagnostic = Diagnostic { violation: &finding.violation }.to_string();
            let diagnostic = match member {
                Some(m) => format!("{m}: {diagnostic}"),
                None    => diagnostic,
            };

            match finding.severity {
                Severity::Error   => self.printer.error(diagnostic),
                Severity::Warning => self.printer.warning(diagnostic),
            }
        }
    }
}

/// A [`Violation`] as a single block of text.
///
/// The message occupies the first line. A hint, when the violation carries one, follows
/// on the next line, indented two spaces and labeled:
///
/// ```text
/// 'psr/log' is in both [require] and [require-dev]
///   hint: keep it in one section
/// ```
///
/// Without a hint, the rendering is just the message.
struct Diagnostic<'a> {
    violation: &'a Violation,
}

#[rustfmt::skip]
impl Display for Diagnostic<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        let Violation { message, hint } = self.violation;

        write!(f, "{message}")?;

        match hint {
            Some(hint) => write!(f, "\n  hint: {hint}"),
            None       => Ok(()),
        }
    }
}
