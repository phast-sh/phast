//! Rendering the workspace root and its members.

use crate::output::Printer;
use crate::styles;

use std::fmt;
use std::fmt::{Display, Formatter};
use std::path::Path;

use phast_workspace::Workspace;

/// Shown in place of a member's `[package].version` when it declares none.
const UNVERSIONED: &str = "?";

/// How `phast workspace` lays out its members.
pub enum ViewMode {
    /// One row per member: name, version, and path relative to the root.
    Flat,
    /// Members with their dependencies as a tree, intra-workspace edges marked.
    ///
    /// Reserved, not implemented. No flag selects it yet, so it is unreachable
    /// until a caller opts in.
    #[allow(dead_code)]
    Tree,
}

/// Renders `phast workspace` in the configured output mode.
pub struct WorkspacePrinter<'a> {
    mode: ViewMode,
    printer: &'a Printer,
}

impl<'a> WorkspacePrinter<'a> {
    pub fn new(mode: ViewMode, printer: &'a Printer) -> Self {
        Self { mode, printer }
    }

    /// Print the workspace root, followed by a blank line.
    pub fn print_root(&self, root: &Path) {
        self.printer.info(format!(
            "Workspace root: {}",
            styles::DEFAULT_VALUE.apply_to(root.display()),
        ));
        self.printer.info("");
    }

    /// Print the member table, or a notice when the workspace has no members.
    ///
    /// `cwd` decides which row is marked as the one being run from.
    ///
    /// The empty case is defensive rather than reachable from `phast workspace`: a workspace
    /// that failed to declare a member never loads, so `Workspace::find` either errors or
    /// hands back one with members.
    pub fn print_members(&self, workspace: &Workspace, cwd: &Path) {
        if !workspace.is_active() {
            self.printer.info("  No members found.");
            return;
        }

        match self.mode {
            ViewMode::Flat => {
                self.printer.info(FlatOutput { workspace, cwd }.to_string());
            }
            ViewMode::Tree => {
                self.printer.info(TreeOutput { workspace, cwd }.to_string());
            }
        }
    }
}

/// Renders the member list as an aligned table, marking the member `cwd` sits in.
struct FlatOutput<'a> {
    cwd: &'a Path,
    workspace: &'a Workspace,
}

#[rustfmt::skip]
impl Display for FlatOutput<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let members = self.workspace.members();

        // Columns are at least as wide as the headings they would carry, so the
        // table keeps its shape for a single short-named member.
        let name_width = members
            .iter()
            .map(|m| m.to_string().len())
            .max()
            .unwrap_or(0)
            .max("name".len());
        let ver_width = members
            .iter()
            .map(|m| m.version().unwrap_or(UNVERSIONED).len())
            .max()
            .unwrap_or(0)
            .max("version".len());

        writeln!(f, "Members ({})", members.len())?;

        let rows: Vec<String> = members
            .iter()
            .map(|member| {
                let name    = member.to_string();
                let version = member.version().unwrap_or(UNVERSIONED);
                let path    = member.path().strip_prefix(self.workspace.root()).unwrap_or(member.path());
                let marker  = if self.cwd.starts_with(member.path()) { "* " } else { "  " };

                format!(
                    "{marker}{:name_width$}  {:ver_width$}  {}",
                    styles::PACKAGE_NAME.apply_to(&name),
                    styles::PACKAGE_VERSION.apply_to(version),
                    styles::PACKAGE_HINT.apply_to(path.display()),
                )
            })
            .collect();

        write!(f, "{}", rows.join("\n"))
    }
}

/// Renders members with their dependencies as a tree, marking intra-workspace edges.
///
/// Reserved alongside [`ViewMode::Tree`]; nothing selects it yet.
struct TreeOutput<'a> {
    cwd: &'a Path,
    workspace: &'a Workspace,
}

impl Display for TreeOutput<'_> {
    fn fmt(&self, _f: &mut Formatter<'_>) -> fmt::Result {
        let _ = (self.cwd, self.workspace);
        todo!("render members and their dependencies as a tree")
    }
}
