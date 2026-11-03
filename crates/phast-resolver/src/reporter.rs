//! Human-readable conflict reporter for the PubGrub solver.
//!
//! Walks the PubGrub derivation tree and extracts a single structured
//! [`Conflict`] value, then formats it in the agreed output style:
//!
//! ```text
//! Phast could not find compatible versions for your requirements
//!  ╰─▶ symfony/polyfill-mbstring has incompatible constraints:
//!        · your project requires symfony/polyfill-mbstring[>=99.0, <100.0]
//!        · symfony/http-foundation[>=7.0, <8.0] requires symfony/polyfill-mbstring[>=1.1, <2.0]
//! ```
//!
//! Tokens that mean nothing to a PHP developer are sanitized on the way out:
//! `<root>` becomes the project, and the `-dev` suffix and the synthetic
//! `0.0.0.0` root version are dropped.

use crate::provider::pubgrub::PubGrubPackage;

use std::fmt::{Display, Formatter, Result};

use phast_version::Version;

use pubgrub::{DefaultStringReporter, DerivationTree, External, Ranges, Reporter};

const ROOT_PKG_LABEL: &str = "your project";

/// Walk the derivation tree and return a formatted conflict report string.
///
/// The tree is collapsed first, which folds away the no-version nodes that would
/// otherwise obscure the requirement that actually clashed.
pub fn report(tree: &mut DerivationTree<PubGrubPackage, Ranges<Version>, String>) -> String {
    tree.collapse_no_versions();
    let conflict = extract_conflict(tree);
    format!("{}", NoSolutionReport { conflict })
}

/// The complete no-solution report: a header and one conflict block.
#[derive(Debug)]
pub struct NoSolutionReport {
    /// The single conflict the walk settled on. A failed solve can involve many,
    /// and reporting the first recognizable one beats listing all of them.
    pub conflict: Conflict,
}

/// One extracted conflict pattern.
#[derive(Debug)]
pub enum Conflict {
    /// Two or more parties require incompatible version ranges of the same package.
    Version(VersionConflict),
    /// A required package no source offers at all.
    NotFound { package: String },
    /// The package exists but no version satisfies the combined constraints.
    NoMatch { package: String, range: String },
    /// A message passed through rather than shaped: PubGrub's own output when the
    /// tree matches no pattern here, or a provider's custom unavailability reason.
    Unknown(String),
}

/// The conflicting package and every party that constrains it.
#[derive(Debug)]
pub struct VersionConflict {
    /// The name of the package whose required versions are incompatible.
    pub package: String,
    /// Every party that places a constraint on `package`, one entry per requirer.
    pub sources: Vec<ConstraintSource>,
}

/// One party that requires the conflicting package at a specific range.
#[derive(Debug, Clone)]
pub struct ConstraintSource {
    /// `"your project"` for root, or `"vendor/pkg[>=x, <y]"` for a dependency.
    pub requirer: String,
    /// The range demanded, e.g. `">=99.0, <100.0"`.
    pub range: String,
}

impl Display for NoSolutionReport {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        writeln!(
            f,
            "Phast could not find compatible versions for your requirements"
        )?;
        write_conflict(f, &self.conflict)
    }
}

fn write_conflict(f: &mut Formatter<'_>, conflict: &Conflict) -> Result {
    match conflict {
        Conflict::Version(conflict) => {
            writeln!(f, " ╰─▶ {} has incompatible constraints:", conflict.package)?;
            for src in &conflict.sources {
                writeln!(
                    f,
                    "       · {} requires {}[{}]",
                    src.requirer, conflict.package, src.range
                )?;
            }
        }
        Conflict::NotFound { package } => {
            writeln!(f, " ╰─▶ {package} is not available on Packagist")?;
        }
        Conflict::NoMatch { package, range } => {
            writeln!(f, " ╰─▶ no version of {package} satisfies [{range}]")?;
        }
        Conflict::Unknown(message) => {
            writeln!(f, " ╰─▶ {message}")?;
        }
    }
    Ok(())
}

/// Walk the derivation tree and extract the first recognizable conflict.
fn extract_conflict(tree: &DerivationTree<PubGrubPackage, Ranges<Version>, String>) -> Conflict {
    if let Some(conflict) = find_conflict(tree) {
        return conflict;
    }

    // Nothing recognized: fall back to PubGrub's default reporter.
    // Sanitize internal tokens that mean nothing to a PHP developer.
    let message = DefaultStringReporter::report(tree)
        .replace("<root>", ROOT_PKG_LABEL)
        .replace("0.0.0.0", "")
        .replace("-dev", "");
    Conflict::Unknown(message)
}

fn find_conflict(
    tree: &DerivationTree<PubGrubPackage, Ranges<Version>, String>,
) -> Option<Conflict> {
    match tree {
        DerivationTree::External(external) => conflict_from_external(external),
        DerivationTree::Derived(derived) => {
            // Try to detect a version conflict at this node first.
            if let Some(conflict) = try_version_conflict(&derived.cause1, &derived.cause2) {
                return Some(Conflict::Version(conflict));
            }
            // Otherwise recurse into both subtrees, left first.
            find_conflict(&derived.cause1).or_else(|| find_conflict(&derived.cause2))
        }
    }
}

/// Try to extract a `VersionConflict` from two child trees.
///
/// Succeeds when both subtrees contain `FromDependencyOf` leaves that
/// constrain the same dependency package, meaning two parties demand
/// incompatible ranges of it.
fn try_version_conflict(
    left: &DerivationTree<PubGrubPackage, Ranges<Version>, String>,
    right: &DerivationTree<PubGrubPackage, Ranges<Version>, String>,
) -> Option<VersionConflict> {
    let mut left_named: Vec<(String, ConstraintSource)> = Vec::new();
    let mut right_named: Vec<(String, ConstraintSource)> = Vec::new();

    collect_named_dep_sources(left, &mut left_named);
    collect_named_dep_sources(right, &mut right_named);

    if left_named.is_empty() || right_named.is_empty() {
        return None;
    }

    // Find the first package name that appears in both sides.
    for (left_name, left_src) in &left_named {
        if let Some((_, right_src)) = right_named
            .iter()
            .find(|(right_name, _)| right_name == left_name)
        {
            return Some(VersionConflict {
                package: left_name.clone(),
                sources: vec![left_src.clone(), right_src.clone()],
            });
        }
    }

    None
}

/// Collect (dependency_package_name, ConstraintSource) pairs from `FromDependencyOf` leaves.
fn collect_named_dep_sources(
    tree: &DerivationTree<PubGrubPackage, Ranges<Version>, String>,
    output: &mut Vec<(String, ConstraintSource)>,
) {
    match tree {
        DerivationTree::External(External::FromDependencyOf(pkg, pkg_range, dep, dep_range)) => {
            let dep_name = package_label(dep).to_string();
            let requirer = format_requirer(pkg, pkg_range);
            let range = format_range(dep_range);
            output.push((dep_name, ConstraintSource { requirer, range }));
        }
        DerivationTree::Derived(derived) => {
            collect_named_dep_sources(&derived.cause1, output);
            collect_named_dep_sources(&derived.cause2, output);
        }
        _ => {}
    }
}

/// Convert a single `External` node to a `Conflict`, if applicable.
fn conflict_from_external(
    ext: &External<PubGrubPackage, Ranges<Version>, String>,
) -> Option<Conflict> {
    match ext {
        External::NoVersions(package, range) => {
            let package = package_label(package).to_string();
            if *range == Ranges::full() {
                Some(Conflict::NotFound { package })
            } else {
                Some(Conflict::NoMatch {
                    package,
                    range: format_range(range),
                })
            }
        }
        // NotRoot is an internal solver marker, not a user-visible conflict.
        External::NotRoot(_, _) => None,
        // Custom unavailability messages are surfaced via Unknown.
        External::Custom(package, _, message) => Some(Conflict::Unknown(format!(
            "{} is unavailable: {}",
            package_label(package),
            message,
        ))),
        // FromDependencyOf is only actionable when the requirer is root.
        // Two sub-cases:
        //   empty range     : root had two incompatible constraints for the same dep,
        //                      pre-intersected to the empty set before PubGrub saw them.
        //   non-empty range : root requires the dep at this range but no published
        //                      version satisfies it (NoVersions collapsed by collapse_no_versions).
        // For non-root requirers one node is not a conflict.
        External::FromDependencyOf(package, _package_range, dependency, dependency_range) => {
            if !matches!(package, PubGrubPackage::Root) {
                return None;
            }
            if *dependency_range == Ranges::empty() {
                Some(Conflict::NoMatch {
                    package: package_label(dependency).to_string(),
                    range: "your project's requirements".to_string(),
                })
            } else {
                Some(Conflict::NoMatch {
                    package: package_label(dependency).to_string(),
                    range: format_range(dependency_range),
                })
            }
        }
    }
}

/// Display a package as `"your project"` (root) or its name.
fn package_label(package: &PubGrubPackage) -> &str {
    match package {
        PubGrubPackage::Root => ROOT_PKG_LABEL,
        PubGrubPackage::Package(name) => name.as_str(),
    }
}

/// Format the requirer side of a `FromDependencyOf` edge.
///
/// Root is shown as `"your project"` with no version.
/// Other packages are shown as `"vendor/pkg[range]"` when constrained,
/// or `"vendor/pkg"` when unconstrained.
fn format_requirer(package: &PubGrubPackage, range: &Ranges<Version>) -> String {
    match package {
        PubGrubPackage::Root => ROOT_PKG_LABEL.to_string(),
        PubGrubPackage::Package(name) => {
            if *range == Ranges::full() {
                name.as_str().to_string()
            } else {
                format!("{}[{}]", name.as_str(), format_range(range))
            }
        }
    }
}

/// Format a version range in bracket notation with `-dev` suffixes stripped.
///
/// `>=1.1, <2.0-dev` becomes `>=1.1, <2.0`.
fn format_range(range: &Ranges<Version>) -> String {
    range.to_string().replace("-dev", "")
}
