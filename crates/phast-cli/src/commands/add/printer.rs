//! Rendering for `phast add`.

use crate::output::Printer;
use crate::project::manifest::{Change, Declared, Section};

/// Reports what `add` changed in one manifest section.
pub struct AddPrinter<'a> {
    /// The section the changes were applied to.
    section: Section,
    /// Where the messages go.
    printer: &'a Printer,
}

impl<'a> AddPrinter<'a> {
    pub fn new(section: Section, printer: &'a Printer) -> Self {
        Self { section, printer }
    }

    /// Report what each change did to the section.
    ///
    /// The section is named when membership changes and left out when it does not:
    /// an author who is told a constraint moved already knows where it lives, but
    /// one adding a package with `--dev` has no other confirmation of where it landed.
    pub fn print(&self, changes: &[Change]) {
        for change in changes {
            match change {
                Change::Insert {
                    package,
                    constraint,
                    replace,
                } => match replace {
                    Declared::Absent => self
                        .printer
                        .info(format!("Added {package} to {}.", self.section.as_str())),
                    Declared::Constraint(replaced) if replaced == constraint => self
                        .printer
                        .info(format!("{package} is already at {constraint}.")),
                    Declared::Constraint(replaced) => self
                        .printer
                        .info(format!("Updated {package}: {replaced} -> {constraint}")),
                    Declared::Inherited => self
                        .printer
                        .info(format!("Updated {package}: workspace -> {constraint}")),
                },
                Change::Inherit { package, replace } => match replace {
                    Declared::Absent => self.printer.info(format!(
                        "Added {package} to {}, inheriting the workspace version.",
                        self.section.as_str(),
                    )),
                    Declared::Constraint(replaced) => self
                        .printer
                        .info(format!("Updated {package}: {replaced} -> workspace")),
                    Declared::Inherited => self
                        .printer
                        .info(format!("{package} already inherits the workspace version.")),
                },
                // `Change` is shared with `phast remove`, the only command that produces
                // this variant. Nothing in `add` can, so there is nothing to report.
                Change::Remove { .. } => continue,
            }
        }
    }
}
