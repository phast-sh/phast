//! Deciding which entries `add` records.

use crate::hints;
use crate::package::PackageRequest;
use crate::project::manifest::{Change, Editor, Section};

use std::collections::HashMap;

use phast_core::package::{PackageName, is_platform};
use phast_resolver::Requirement;
use phast_version::{Constraint, RequireSpec, Version};
use phast_workspace::{Member, Workspace};

use anyhow::{Context, Result, anyhow, bail};

/// A constraint the workspace already answers, so no registry is asked for it.
pub enum Known {
    /// The root pins it in `[workspace.dependencies]`. The member records
    /// `{ workspace = true }` rather than spelling a constraint the root owns.
    Pinned(String),
    /// Another member of the same workspace declares it, and this is `^major.minor` of the
    /// version that member declares. It is satisfied from disk, so it is never solved for.
    Sibling(String),
}

impl Known {
    /// The constraint to solve against, whatever the member ends up recording.
    fn constraint(&self) -> &str {
        match self {
            Self::Pinned(constraint) | Self::Sibling(constraint) => constraint,
        }
    }

    /// Whether another member of the same workspace provides it.
    fn is_sibling(&self) -> bool {
        matches!(self, Self::Sibling(_))
    }
}

/// What the workspace answers for each requested package, by name.
///
/// Taken before the workspace moves into the installer, because the change planning that
/// needs it runs from inside `Installer::add`. Narrowed to the requested packages so what
/// travels is the answer to this invocation's question, not the whole shared table.
pub fn known(requests: &[PackageRequest], workspace: &Workspace) -> Result<HashMap<String, Known>> {
    let siblings: HashMap<&str, &Member> = workspace
        .members()
        .iter()
        .filter_map(|m| m.name().map(|name| (name, m)))
        .collect();

    let mut known = HashMap::new();
    for request in requests {
        // The root's pin comes first: a package the root pins is inherited even
        // when it is also a member, so only the root decides what every member gets.
        if let Some(pinned) = workspace.dependencies().get(&request.name) {
            known.insert(request.name.clone(), Known::Pinned(pinned.to_owned()));
            continue;
        }

        // Not a member of this workspace, so it is an ordinary package and a registry
        // is the right place to ask.
        let Some(sibling) = siblings.get(request.name.as_str()) else {
            continue;
        };

        // A member without one has nothing to derive from, and inventing a version would be
        // worse than saying so: that is a manifest to fix.
        let version = sibling
            .version()
            .ok_or_else(|| anyhow!("{}", hints::member_without_version(&request.name)))?;

        let version = version
            .parse()
            .with_context(|| format!("failed to read the version of '{}'", request.name))?;

        known.insert(request.name.clone(), Known::Sibling(caret(&version)));
    }

    Ok(known)
}

/// The changes to apply, refusing any package the opposite section already declares.
///
/// A refusal discards the whole set. Changes for earlier requests are built first and then
/// dropped when collecting into a `Result`, so naming one package that cannot be added
/// leaves the others unwritten rather than half-written. That holds only while building a
/// change stays free of side effects.
///
/// `selected` carries the versions the solver chose for requests that arrived without a
/// constraint. A request with neither a constraint nor a selected version is an error.
pub fn changes(
    requests: &[PackageRequest],
    manifest: &Editor,
    section: Section,
    selected: &HashMap<PackageName, Version>,
    known: &HashMap<String, Known>,
) -> Result<Vec<Change>> {
    let opposite = section.opposite();

    requests
        .iter()
        .map(|request| {
            if manifest.contains(opposite, &request.name) {
                bail!(hints::package_already_exists(&request.name, &opposite));
            }

            // What the section records today, which is what makes this an update rather
            // than an addition.
            let replace = manifest.declares(section, &request.name);
            let package = request.name.clone();

            let constraint = match known.get(&request.name) {
                // The root owns this one. A constraint that disagrees is refused rather
                // than narrowed: only the root can change what every member gets.
                Some(Known::Pinned(pinned)) => {
                    if declared(request).is_some_and(|given| given != *pinned) {
                        bail!(hints::constraint_pinned_by_workspace(&request.name, pinned));
                    }

                    return Ok(Change::Inherit { package, replace });
                }

                // A sibling records an ordinary constraint, derived from the version that
                // member declares. One given by hand wins: it is the more specific answer.
                Some(Known::Sibling(derived)) => {
                    declared(request).unwrap_or_else(|| derived.clone())
                }

                // Nothing the workspace answers, so the member records one of its own.
                None => constraint(request, selected),
            };

            Ok(Change::Insert {
                package,
                constraint,
                replace,
            })
        })
        .collect()
}

/// The requirements to resolve, or `None` when every constraint is already known.
///
/// `None` is what keeps `add psr/log:^3.0` offline: there is nothing to choose, so there
/// is nothing to ask a registry. A package the root pins is known for the same reason.
/// Otherwise, every request is included, not only the ones missing a constraint, because
/// none of them are in the manifest yet and the solution has to hold for all of them at once.
pub fn requirements(
    requests: &[PackageRequest],
    known: &HashMap<String, Known>,
    dev: bool,
) -> Result<Option<Vec<Requirement>>> {
    let constraints = constraints(requests, known);
    if constraints.iter().all(Option::is_some) {
        return Ok(None);
    }

    // A sibling is satisfied from disk, so it is not something to solve for. Proposing it
    // would send the solver to a registry, which is the one place it cannot be found.
    let proposed = requests
        .iter()
        .zip(constraints)
        .filter(|(request, _)| !known.get(&request.name).is_some_and(Known::is_sibling));

    proposed
        .map(|(request, constraint)| {
            Ok(Requirement {
                name: PackageName::new(&request.name),
                spec: spec(constraint)?,
                is_dev: dev,
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

/// Per request, the constraint that needs no version chosen, in the order given.
///
/// Either the request carries one or the workspace answers it. What the workspace answers is
/// used for the solve only; what gets written for it is [`Known`]'s business.
fn constraints(requests: &[PackageRequest], known: &HashMap<String, Known>) -> Vec<Option<String>> {
    requests
        .iter()
        .map(|request| {
            declared(request).or_else(|| {
                known
                    .get(&request.name)
                    .map(|known| known.constraint().to_owned())
            })
        })
        .collect()
}

/// The constraint to record for `request`.
///
/// Resolution has already succeeded by the time this runs, and it fails on any requirement
/// nothing accounts for, so a package absent from `selected` is one another package carries
/// through `provide` or `replace`.
fn constraint(request: &PackageRequest, selected: &HashMap<PackageName, Version>) -> String {
    if let Some(constraint) = declared(request) {
        return constraint;
    }

    match selected.get(&PackageName::new(&request.name)) {
        Some(version) => caret(version),
        // Carried rather than installed, so there is no version to derive from and none to
        // track: an interface is a statement about what must be available, the same as a
        // platform package.
        None => "*".to_owned(),
    }
}

/// The constraint a request carries on its own, if any.
///
/// A platform package describes the environment rather than a version range to track, and
/// has no registry versions to choose from, so it takes `*` when none was given.
fn declared(request: &PackageRequest) -> Option<String> {
    request
        .constraint
        .clone()
        .or_else(|| is_platform(&request.name).then(|| "*".to_owned()))
}

/// `^major.minor` of a selected version: `3.9.2` becomes `^3.9`.
///
/// The patch level is dropped because pinning it would defeat the caret, and the exact
/// version is recorded in `phast.lock`, which is where an exact version belongs. A branch
/// version has no major or minor to derive from, so it is recorded as itself.
fn caret(version: &Version) -> String {
    match version {
        Version::Numeric(n) => format!("^{}.{}", n.segments[0], n.segments[1]),
        Version::Branch(_) => version.to_string(),
    }
}

/// What to solve for, given the constraint already settled for a request.
///
/// An absent constraint becomes unbounded: the solver is told only that
/// the package is required, and every other rule in the manifest decides the rest.
fn spec(constraint: Option<String>) -> Result<RequireSpec> {
    let Some(constraint) = constraint else {
        return Ok(RequireSpec::Constraint {
            constraint: Constraint::Any,
            stability: None,
        });
    };

    Ok(constraint.parse()?)
}
