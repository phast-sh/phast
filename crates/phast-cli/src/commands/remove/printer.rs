//! Rendering for `phast remove`.

use crate::output::Printer;
use crate::project::manifest::{Change, Section};

/// Reports what `remove` dropped from one manifest section.
pub struct RemovePrinter<'a> {
    section: Section,
    printer: &'a Printer,
}

impl<'a> RemovePrinter<'a> {
    pub fn new(section: Section, printer: &'a Printer) -> Self {
        Self { section, printer }
    }

    /// Name every entry dropped, and the section it came from.
    pub fn print(&self, changes: &[Change]) {
        for change in changes {
            self.printer.info(format!(
                "Removed {} from {}.",
                change.package(),
                self.section.as_str(),
            ));
        }
    }
}
