//! Rendering the `[scripts]` listing shown by a bare `phast run`.

use crate::output::Printer;
use crate::styles;

use std::collections::BTreeMap;
use std::fmt;
use std::fmt::{Display, Formatter};

use phast_core::manifest::Script;

/// Renders the script listing, which is all a bare `phast run` prints.
///
/// A running script's own output never reaches this type: the child process inherits stdout
/// and stderr, so only the echoed `> pwd` line is printed by Phast.
pub struct RunPrinter<'a> {
    printer: &'a Printer,
}

impl<'a> RunPrinter<'a> {
    pub fn new(printer: &'a Printer) -> Self {
        Self { printer }
    }

    /// Print every script in `[scripts]`, or a notice when the section is empty.
    pub fn print(&self, scripts: &BTreeMap<String, Script>) {
        if scripts.is_empty() {
            self.printer.info("No scripts defined in [scripts].");
            return;
        }

        self.printer.info(ScriptsOutput { scripts }.to_string());
    }
}

/// Renders the scripts as an aligned two-column table of name and command.
///
/// A multi-command script shows its first command only;
/// `phast run <name>` runs the whole list.
struct ScriptsOutput<'a> {
    scripts: &'a BTreeMap<String, Script>,
}

#[rustfmt::skip]
impl Display for ScriptsOutput<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let name_width = self.scripts
            .keys()
            .map(String::len)
            .max()
            .unwrap_or(0);

        // `scripts` is a BTreeMap, so this is already in name order.
        let rows: Vec<String> = self.scripts
            .iter()
            .map(|(name, script)| {
                let command = match script {
                    Script::Single(s)   => s.as_str(),
                    Script::Multiple(m) => m.first().map(String::as_str).unwrap_or(""),
                };

                format!(
                    "  {:name_width$}  {}",
                    styles::DEFAULT_KEY.apply_to(name),
                    styles::DEFAULT_VALUE.apply_to(command),
                )
            })
            .collect();

        write!(f, "{}", rows.join("\n"))
    }
}
