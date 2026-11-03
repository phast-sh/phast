//! Rendering `phast fmt` output.

use crate::output::Printer;

use similar::{ChangeTag, TextDiff};

/// Prints which manifests would change, and under `--check` how.
pub struct FmtPrinter<'a> {
    printer: &'a Printer,
}

impl<'a> FmtPrinter<'a> {
    pub fn new(printer: &'a Printer) -> Self {
        Self { printer }
    }

    /// Report a manifest that is not formatted.
    ///
    /// `name` is absent for a standalone project, where one file needs no distinguishing.
    /// `changes` is the pair to diff, present only under `--check`: a run that rewrites the
    /// file has nothing to show, since the file itself is the output.
    pub fn print(&self, name: Option<&str>, changes: Option<(&str, &str)>) {
        if let Some(name) = name {
            self.printer.info(name);
        }

        if let Some((original, formatted)) = changes {
            self.diff(original, formatted);
        }
    }

    /// A colored line diff between `original` and `formatted`.
    ///
    /// Deletions are red, insertions are green, unchanged lines are omitted.
    #[rustfmt::skip]
    fn diff(&self, original: &str, formatted: &str) {
        let diff = TextDiff::from_lines(original, formatted);
        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Delete => print!(
                    "\x1b[31m- {:>4} {change}\x1b[0m",
                    change.old_index().unwrap_or(0) + 1,
                ),
                ChangeTag::Insert => print!(
                    "\x1b[32m+ {:>4} {change}\x1b[0m",
                    change.new_index().unwrap_or(0) + 1,
                ),
                ChangeTag::Equal => {}
            }
        }
    }
}
