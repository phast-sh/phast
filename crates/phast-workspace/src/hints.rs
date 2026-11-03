//! User-facing error messages for workspace operations.
//!
//! Centralised here so all workspace errors use the same wording and tone.

use std::path::Path;

/// --member was supplied but no workspace is active.
pub fn member_requires_workspace() -> &'static str {
    "--member requires a workspace. Add a [workspace] section to phast.toml first"
}

/// --member named a path that is not a member.
///
/// `path` is echoed as the user typed it, and it is a path relative to the workspace
/// root. Output identifies members by `[package].name`, so a name copied from there,
/// `acme/core` for the member in `packages/core`, arrives here rather than matching.
pub fn member_not_found(path: &str) -> String {
    format!("'{}' is not a workspace member", path)
}

/// `load` was pointed at a directory whose `phast.toml` declares no `[workspace]`.
pub fn not_a_workspace_root(root: &Path) -> String {
    format!(
        "'{}' is not a workspace root: its phast.toml has no [workspace] section",
        root.display(),
    )
}

/// Workspace root declares a `[package]` section, which is forbidden.
pub fn no_root_package_allowed() -> &'static str {
    "workspace root must not declare a [package] section; \
     move the package into a member directory or remove [package] from the root"
}

/// Workspace root declares `[require]` or `[require-dev]`, which is forbidden.
pub fn no_root_require_allowed(section: &str) -> String {
    format!(
        "workspace root must not declare [{section}]; \
         declare shared versions in [workspace.dependencies] \
         and let members opt in with {{ workspace = true }}",
    )
}

/// No `[workspace].members` patterns are declared in the workspace root.
pub fn no_members_declared() -> &'static str {
    "workspace [workspace].members is empty; add at least one member pattern"
}

/// The declared member patterns matched no directories containing a `phast.toml`.
pub fn no_members_found(patterns: &[String]) -> String {
    format!(
        "no phast.toml files found matching workspace patterns {:?}",
        patterns,
    )
}

/// A `{ workspace = true }` ref reached the merge without being expanded.
///
/// Unlike [`unresolved_workspace_ref`], this is not something a manifest can ask for:
/// expansion either rewrites every ref or fails, so reaching the merge with one intact
/// means expansion was skipped.
pub fn unexpanded_workspace_ref(package: &str, member: &str) -> String {
    format!("'{package}' in '{member}' has an unresolved {{ workspace = true }} ref")
}

/// A member uses `{ workspace = true }` for a package not listed in `[workspace.dependencies]`.
pub fn unresolved_workspace_ref(package: &str) -> String {
    format!(
        "'{}' uses {{ workspace = true }} but is not defined in [workspace.dependencies]",
        package,
    )
}

/// A member requires a sibling whose declared version does not satisfy the constraint.
pub fn sibling_version_mismatch(
    requirer: &str,
    package: &str,
    constraint: &str,
    version: &str,
) -> String {
    format!(
        "{requirer} requires {package} \"{constraint}\", \
         but workspace member {package} declares version {version}\n\n  \
         hint: align the constraint with the member version, or bump the member",
    )
}

/// A member requires a sibling that declares no `[package].version`.
pub fn sibling_version_missing(requirer: &str, package: &str) -> String {
    format!(
        "{requirer} requires workspace member {package}, which declares no version\n\n  \
         hint: add version = \"...\" to [package] in the {package} manifest",
    )
}

/// Workspace members form a cycle through their production `require` edges.
///
/// `cycle` is the path of member names, ending where it started
/// (e.g. `["acme/a", "acme/b", "acme/a"]`).
pub fn require_cycle(cycle: &[&str]) -> String {
    format!(
        "workspace members form a require cycle: {}.\n  \
         hint: break the cycle, or move one edge to [require-dev]",
        cycle.join(" -> "),
    )
}

/// Two workspace members spell the same external package's constraint differently.
///
/// Differently, not incompatibly: `^1.0` and `>=1.0 <2.0` conflict here even though
/// they admit the same versions.
#[rustfmt::skip]
pub fn dependency_conflict(
    package: &str,
    constraint_a: &str, member_a: &str,
    constraint_b: &str, member_b: &str,
) -> String {
    conflict(
        package,
        constraint_a, member_a,
        constraint_b, member_b,
        "pin a shared constraint in [workspace.dependencies] to resolve this",
    )
}

/// The same conflict, over a package the root already pins.
///
/// Advising a pin that exists names the wrong file to open: the member is what disagrees.
#[rustfmt::skip]
pub fn pinned_dependency_conflict(
    package: &str,
    constraint_a: &str, member_a: &str,
    constraint_b: &str, member_b: &str,
) -> String {
    conflict(
        package,
        constraint_a, member_a,
        constraint_b, member_b,
        "[workspace.dependencies] already pins this; inherit it with { workspace = true }",
    )
}

/// Both conflicts, which differ only in the advice they end with.
#[rustfmt::skip]
fn conflict(
    package: &str,
    constraint_a: &str, member_a: &str,
    constraint_b: &str, member_b: &str,
    hint: &str,
) -> String {
    format!(
        "workspace dependency conflict for \"{package}\"\n  \
         {member_a} requires \"{constraint_a}\"\n  \
         {member_b} requires \"{constraint_b}\"\n\n  \
         hint: {hint}",
    )
}
