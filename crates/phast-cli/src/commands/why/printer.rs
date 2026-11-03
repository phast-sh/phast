//! Rendering a `phast why` result as a flat table or as a tree.
//!
//! Neither view contains the other, so `--tree` is not the flat answer with more of it. The
//! table carries the constraint and the section it was declared in, `^1.0  [require]`, and
//! no version. The tree carries the locked version and the depth, `acme/logger 0.0.0`, and
//! no constraint.
//!
//! They also name the root differently: the table prints `root (your project)`, which comes
//! from `Requirer::as_str`, while the tree prints `required by root` from here.

use super::checker::{Requirer, TreeNode, WhyResult};

use crate::output::Printer;
use crate::styles;

use std::fmt;
use std::fmt::{Display, Formatter};

/// How a `phast why` result is laid out.
#[derive(Clone, Copy)]
pub enum ViewMode {
    /// Show only direct requirers (default).
    Flat,
    /// Show the full reverse-dependency tree toward root.
    Tree,
}

/// Renders the why result in the configured output mode.
pub struct WhyPrinter<'a> {
    format: ViewMode,
    printer: &'a Printer,
}

impl<'a> WhyPrinter<'a> {
    /// Create a printer that will render in the given output mode.
    pub fn new(format: ViewMode, printer: &'a Printer) -> Self {
        Self { format, printer }
    }

    /// Render `result` for `package` at `version` using the configured mode.
    pub fn print(&self, package: &str, version: &str, result: &WhyResult) {
        match self.format {
            ViewMode::Flat => {
                self.printer.info(
                    FlatOutput {
                        package,
                        version,
                        requirers: &result.requirers,
                    }
                    .to_string(),
                );
            }
            ViewMode::Tree => {
                self.printer.info(
                    TreeOutput {
                        package,
                        version,
                        nodes: &result.nodes,
                    }
                    .to_string(),
                );
            }
        }
    }
}

/// Renders a `phast why` result as an aligned table of direct requirers.
struct FlatOutput<'a> {
    package: &'a str,
    version: &'a str,
    requirers: &'a [Requirer],
}

#[rustfmt::skip]
impl Display for FlatOutput<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} {} is required by:\n",
            styles::PACKAGE_NAME.apply_to(self.package),
            styles::PACKAGE_VERSION.apply_to(self.version),
        )?;

        if self.requirers.is_empty() {
            return write!(f, "  (not required by any installed package)");
        }

        let name_width = self.requirers
            .iter()
            .map(|r| r.as_str().len())
            .max()
            .unwrap_or(0);
        let constraint_width = self.requirers.iter()
            .map(|r| r.constraint.len())
            .max()
            .unwrap_or(0);

        for requirer in self.requirers {
            let section = if requirer.dev { "[require-dev]" } else { "[require]" };
            writeln!(f, "  {:name_width$}  {:constraint_width$}  {}",
                styles::PACKAGE_NAME.apply_to(requirer.as_str()),
                styles::PACKAGE_VERSION.apply_to(&requirer.constraint),
                styles::PACKAGE_HINT.apply_to(section),
            )?;
        }
        Ok(())
    }
}

/// Renders a `phast why` result as a recursive tree with box-drawing characters.
struct TreeOutput<'a> {
    package: &'a str,
    version: &'a str,
    nodes: &'a [TreeNode],
}

#[rustfmt::skip]
impl Display for TreeOutput<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} {}",
            styles::PACKAGE_NAME.apply_to(self.package),
            styles::PACKAGE_VERSION.apply_to(self.version),
        )?;

        let mut output = String::new();
        render_nodes(self.nodes, "", &mut output);

        if output.is_empty() {
            write!(f, "  (not required by any installed package)")
        } else {
            write!(f, "{}", output.trim_end())
        }
    }
}

/// Render `nodes` into `output` using box-drawing characters, recursing for children.
///
/// `prefix` carries the current indentation string (`"│   "` or `"    "`) built up
/// by parent calls, ensuring connectors align correctly at every nesting level.
#[rustfmt::skip]
fn render_nodes(nodes: &[TreeNode], prefix: &str, output: &mut String) {
    for (index, node) in nodes.iter().enumerate() {
        let is_last      = index == nodes.len() - 1;
        let connector    = if is_last { "└── " } else { "├── " };
        let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });

        match node {
            TreeNode::Root { dev } => {
                let label = if *dev { " (dev)" } else { "" };
                let text  = styles::PACKAGE_HINT.apply_to(format!("required by root{label}"));
                output.push_str(&format!("{prefix}{connector}{text}\n"));
            }
            TreeNode::Package { name, version, dev, children } => {
                let label = if *dev { " (dev)" } else { "" };
                output.push_str(&format!("{prefix}{connector}{} {}{label}\n",
                    styles::PACKAGE_NAME.apply_to(name),
                    styles::PACKAGE_VERSION.apply_to(version),
                ));
                render_nodes(children, &child_prefix, output);
            }
            TreeNode::Circular => {
                output.push_str(&format!("{prefix}{connector}(circular)\n"));
            }
        }
    }
}
