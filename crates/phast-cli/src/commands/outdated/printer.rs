//! Rendering outdated packages as text or JSON.

use super::checker::{OutdatedEntry, UpdateKind};

use crate::output::{OutputFormat, Printer};
use crate::styles;

use std::fmt;
use std::fmt::{Display, Formatter};

use anyhow::{Context, Result};
use serde::Serialize;

/// Renders a list of outdated packages in the requested output format.
pub struct OutdatedPrinter<'a> {
    format: OutputFormat,
    printer: &'a Printer,
}

impl<'a> OutdatedPrinter<'a> {
    pub fn new(format: OutputFormat, printer: &'a Printer) -> Self {
        Self { format, printer }
    }

    pub fn print(&self, outdated: &[OutdatedEntry]) -> Result<()> {
        match self.format {
            OutputFormat::Text => self.print_text(outdated),
            OutputFormat::Json => self.print_json(outdated),
        }
    }

    #[rustfmt::skip]
    fn print_text(&self, outdated: &[OutdatedEntry]) -> Result<()> {
        self.printer.info(TextOutput { outdated }.to_string());
        Ok(())
    }

    #[rustfmt::skip]
    fn print_json(&self, outdated: &[OutdatedEntry]) -> Result<()> {
        self.printer.info(JsonOutput { outdated }.to_pretty_json()?);
        Ok(())
    }
}

/// Renders a slice of outdated entries as human-readable text.
struct TextOutput<'a> {
    outdated: &'a [OutdatedEntry],
}

#[rustfmt::skip]
impl Display for TextOutput<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.outdated.is_empty() {
            return write!(f, "All packages are up to date.");
        }

        let noun = if self.outdated.len() == 1 { "package" } else { "packages" };
        writeln!(f, "Outdated {} ({}):\n", noun, self.outdated.len())?;

        let name_width = self.outdated
            .iter()
            .map(|e| e.package.len())
            .max()
            .unwrap_or(0);
        let locked_width = self.outdated
            .iter()
            .map(|e| e.locked.len())
            .max()
            .unwrap_or(0);
        let latest_width = self.outdated
            .iter()
            .map(|e| e.latest.len())
            .max()
            .unwrap_or(0);

        let major_style = styles::UPDATE_MAJOR;
        let minor_style = styles::UPDATE_MINOR;
        let patch_style = styles::UPDATE_PATCH;

        for entry in self.outdated {
            let kind_styled = match entry.kind {
                UpdateKind::Major => major_style.apply_to(entry.kind.as_str()).to_string(),
                UpdateKind::Minor => minor_style.apply_to(entry.kind.as_str()).to_string(),
                UpdateKind::Patch => patch_style.apply_to(entry.kind.as_str()).to_string(),
            };

            writeln!(f,
                "  {:name_width$}  {:locked_width$}  {}  {:latest_width$}  {}",
                styles::PACKAGE_NAME.apply_to(&entry.package),
                styles::PACKAGE_VERSION.apply_to(&entry.locked),
                styles::PACKAGE_HINT.apply_to("→"),
                styles::PACKAGE_VERSION.apply_to(&entry.latest),
                kind_styled,
            )?;
        }

        write!(f, "\nRun `phast update` to update within constraints.")
    }
}

/// Renders a slice of outdated entries as a pretty-printed JSON object.
///
/// Output shape: `{ "outdated": [ ... ] }`
#[derive(Serialize)]
struct JsonOutput<'a> {
    outdated: &'a [OutdatedEntry],
}

#[rustfmt::skip]
impl JsonOutput<'_> {
    fn to_pretty_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .context("failed to serialize outdated results as JSON")
    }
}
