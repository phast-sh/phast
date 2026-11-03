//! Rendering advisories as text or JSON.

use super::advisory::{Advisory, Severity};

use crate::output::{OutputFormat, Printer};
use crate::styles;

use std::collections::BTreeMap;
use std::fmt;

use anyhow::{Context, Result};
use console::Style;
use serde::Serialize;

/// Terminal color style for the severity bracket in text output.
///
/// A free function rather than a method on `Severity` so the model stays free of
/// terminal concerns.
#[rustfmt::skip]
fn severity_style(severity: &Severity) -> Style {
    match severity {
        Severity::Critical => styles::SEVERITY_CRITICAL,
        Severity::High     => styles::SEVERITY_HIGH,
        Severity::Medium   => styles::SEVERITY_MEDIUM,
        Severity::Low      => styles::SEVERITY_LOW,
        Severity::Unknown  => styles::SEVERITY_UNKNOWN,
    }
}

/// Renders a slice of advisories as human-readable text.
///
/// Packages are grouped alphabetically. Within each group advisories are
/// listed with aligned ID, colored severity bracket, and title columns.
/// Up to three detail lines follow each advisory, indented to align under the title:
/// affected versions, report date, and URL. Each is omitted when the advisory has
/// nothing to put there.
struct TextOutput<'a> {
    advisories: &'a [Advisory],
}

#[rustfmt::skip]
impl fmt::Display for TextOutput<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.advisories.is_empty() {
            return writeln!(f, "No security advisories found in your locked dependencies.");
        }

        let total = self.advisories.len();
        let noun  = if total == 1 { "advisory" } else { "advisories" };
        writeln!(f, "Phast found {total} security {noun} in your locked dependencies.\n")?;

        let id_width = self.advisories
            .iter()
            .map(|a| a.id.len())
            .max()
            .unwrap_or(0);
        let sev_width = self.advisories
            .iter()
            .map(|a| a.severity.as_str().len() + 2)
            .max()
            .unwrap_or(0);

        let mut by_package: BTreeMap<&str, Vec<&Advisory>> = BTreeMap::new();
        for advisory in self.advisories {
            by_package
                .entry(advisory.package.as_str())
                .or_default()
                .push(advisory);
        }

        for (package, group) in &by_package {
            let version = group[0].version.as_str();
            writeln!(f, "  {} {}",
                styles::PACKAGE_NAME.apply_to(package),
                styles::PACKAGE_VERSION.apply_to(version),
            )?;

            for advisory in group.iter() {
                let sev_label  = format!("[{}]", advisory.severity.as_str());
                // Pre-pad before styling so ANSI codes don't corrupt column alignment.
                let padded_id  = format!("{:<id_width$}", advisory.id);
                let padded_sev = format!("{:<sev_width$}", sev_label);
                writeln!(f, "    {}  {}  {}",
                    styles::ADVISORY_ID.apply_to(&padded_id),
                    severity_style(&advisory.severity).apply_to(&padded_sev),
                    advisory.title,
                )?;

                let indent = " ".repeat(4 + id_width + 2 + sev_width + 2);

                if !advisory.affected_versions.is_empty() {
                    writeln!(f, "{}{}  {}", indent,
                        styles::DEFAULT_KEY.apply_to(format!("{:<7}", "affected")),
                        styles::DEFAULT_VALUE.apply_to(&advisory.affected_versions))?;
                }

                if let Some(date) = advisory.reported_at {
                    writeln!(f, "{}{}  {}", indent,
                        styles::DEFAULT_KEY.apply_to(format!("{:<7}", "reported")),
                        styles::DEFAULT_VALUE.apply_to(date.format("%Y-%m-%d").to_string()))?;
                }

                if !advisory.url.is_empty() {
                    writeln!(f, "{}{}", indent, styles::PACKAGE_HINT.apply_to(&advisory.url))?;
                }
            }
            writeln!(f)?;
        }

        write!(f, "Run `phast update` to attempt resolution.")
    }
}

/// Renders a slice of advisories as a pretty-printed JSON object.
///
/// Output shape: `{ "advisories": [ ... ] }`
#[derive(Serialize)]
struct JsonOutput<'a> {
    advisories: &'a [Advisory],
}

#[rustfmt::skip]
impl JsonOutput<'_> {
    fn to_pretty_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .context("failed to serialize audit results as JSON")
    }
}

/// Renders a list of advisories in the requested output format.
pub struct AdvisoryPrinter<'a> {
    format: OutputFormat,
    printer: &'a Printer,
}

impl<'a> AdvisoryPrinter<'a> {
    pub fn new(format: OutputFormat, printer: &'a Printer) -> Self {
        Self { format, printer }
    }

    /// Print advisories in the configured format.
    pub fn print(&self, advisories: &[Advisory]) -> Result<()> {
        match self.format {
            OutputFormat::Text => self.print_text(advisories),
            OutputFormat::Json => self.print_json(advisories),
        }
    }

    #[rustfmt::skip]
    fn print_text(&self, advisories: &[Advisory]) -> Result<()> {
        self.printer.info(TextOutput { advisories }.to_string());
        Ok(())
    }

    #[rustfmt::skip]
    fn print_json(&self, advisories: &[Advisory]) -> Result<()> {
        self.printer.info(JsonOutput { advisories }.to_pretty_json()?);
        Ok(())
    }
}
