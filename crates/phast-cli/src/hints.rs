//! User-facing error messages and hints for CLI commands.
//!
//! Centralised here so all commands use the same wording and tone. Every function returns a
//! plain string and stops there. Whether it becomes a `bail!`, an `anyhow!`, a warning or a
//! success line is the call site's decision, so this module carries the wording and none of
//! the severity.

use crate::project::manifest::Section;

/// The package name is not a recognized format.
pub fn invalid_package_name(name: &str) -> String {
    format!(
        "invalid package name '{}'. Expected vendor/package (e.g. `psr/log`), `php`, or `ext-*` (e.g. `ext-json`).",
        name,
    )
}

/// The package already exists in the opposite (require, require-dev) section.
#[rustfmt::skip]
pub fn package_already_exists(name: &str, section: &Section) -> String {
    let hint = match section {
        Section::Require    => format!("Remove it first with `phast remove {}`.", name),
        Section::RequireDev => format!("Remove it first with `phast remove --dev {}`, or add --dev to move it.", name),
    };
    format!("'{}' is already in {}. {}", name, section.as_str(), hint)
}

/// The package was not found in the targeted section.
#[rustfmt::skip]
pub fn package_not_found(name: &str, section: &Section) -> String {
    let hint = match section {
        Section::Require    => "If it is a dev dependency, use --dev.",
        Section::RequireDev => "Remove --dev to target require.",
    };
    format!("'{}' is not in {}. {}", name, section.as_str(), hint)
}

/// The package was not found in phast.lock.
pub fn package_not_in_lock(name: &str) -> String {
    format!("package `{}` is not in phast.lock", name)
}

/// No packages were passed to `phast add`.
pub fn no_packages_to_add() -> &'static str {
    "no packages specified. Usage: phast add vendor/package:^1.0"
}

/// No packages were passed to `phast remove`.
pub fn no_packages_to_remove() -> &'static str {
    "no packages specified. Usage: phast remove vendor/package"
}

/// phast.toml already exists and --force was not passed.
pub fn phast_toml_exists() -> &'static str {
    "'phast.toml' already exists. Use --force to overwrite."
}

/// phast.toml was not found in the current directory.
pub fn phast_toml_not_found() -> &'static str {
    "phast.toml not found. Run `phast init` first."
}

/// phast.lock was not found in the current directory.
pub fn phast_lock_not_found() -> &'static str {
    "phast.lock not found. Run `phast install` first."
}

/// The named script was not found in `[scripts]`.
pub fn script_not_found(name: &str) -> String {
    format!("script '{}' not found in [scripts].", name)
}

/// composer.json was not found in the current directory.
pub fn composer_json_not_found() -> &'static str {
    "'composer.json' not found in current directory"
}

/// Manifests `phast fmt` rewrote. See [`manifests_not_formatted`] on the wording.
pub fn manifests_formatted(formatted: usize, examined: usize) -> String {
    if examined == 1 {
        return "phast.toml formatted.".to_owned();
    }

    format!("{formatted} manifests formatted.")
}

/// Manifests that would change under `phast fmt --check`.
///
/// `examined` decides the wording, not `unformatted`: a project with one manifest is named,
/// and a project with several is counted. One unformatted member out of three is still a
/// count, which is why the number cannot decide it.
pub fn manifests_not_formatted(unformatted: usize, examined: usize) -> String {
    if examined == 1 {
        return "phast.toml is not formatted. Run `phast fmt` to fix.".to_owned();
    }

    format!("{unformatted} manifests are not formatted. Run `phast fmt` to fix.")
}

/// Nothing for `phast fmt` to do. See [`manifests_not_formatted`] on the wording.
pub fn manifests_already_formatted(examined: usize) -> &'static str {
    if examined == 1 {
        return "phast.toml is already formatted, nothing to do.";
    }

    "Every manifest is already formatted, nothing to do."
}

/// Nothing for `phast validate` to report, with any warnings it tolerated.
///
/// `examined` decides the wording the same way it does for `phast fmt`: a project with one
/// manifest is named, and a project with several is spoken of collectively.
pub fn manifests_are_valid(warnings: usize, examined: usize) -> String {
    let subject = if examined == 1 {
        "phast.toml"
    } else {
        "Every manifest"
    };

    match warnings {
        0 => format!("{subject} is valid."),
        n => format!("{subject} is valid ({n} warning(s))."),
    }
}

/// Errors found by `phast validate`, across every manifest it checked.
pub fn manifests_have_errors(errors: usize) -> String {
    format!("{errors} error(s) found.")
}

/// Warnings found by `phast validate`, which fail the run only under `--strict`.
pub fn manifests_have_warnings(warnings: usize) -> String {
    format!("{warnings} warning(s) found.")
}

/// No PHP runtime could be detected and no constraint was given.
pub fn php_version_required() -> &'static str {
    "could not detect a PHP version. Pass one with --php, e.g. `--php ^8.4`."
}

/// No workspace root was found walking up from the current directory.
pub fn workspace_not_found() -> &'static str {
    "no workspace found. Add a [workspace] section to phast.toml to create one."
}

/// --member resolved to a member with no declared `[package].name` to scope to.
pub fn member_without_name(path: &str) -> String {
    format!(
        "workspace member '{}' has no [package].name to scope to",
        path,
    )
}

/// A workspace member was added by name, but declares no version to derive a constraint from.
pub fn member_without_version(name: &str) -> String {
    format!(
        "workspace member '{name}' declares no [package].version to derive a constraint from. \
        Add one, or pass a constraint: {name}:^1.0",
    )
}

/// A dependency was addressed at a workspace root, which declares none of its own.
pub fn member_required_for_dependencies() -> &'static str {
    "a workspace root has no dependencies of its own. Target a member with --member <path>"
}

/// An explicit constraint disagrees with the root's `[workspace.dependencies]` pin.
pub fn constraint_pinned_by_workspace(name: &str, pinned: &str) -> String {
    format!(
        "the workspace pins '{name}' at \"{pinned}\". \
        Drop the constraint to inherit it, or change the pin at the root",
    )
}

/// A package has no integrity checksum, so it cannot be verified.
pub fn missing_checksum(name: &str) -> String {
    format!(
        "'{}' has no checksum: the registry published none, so it cannot be verified.",
        name,
    )
}

/// A version `minimum-package-age` rejected in a resolve that then failed, and the date it
/// is offered from.
pub fn rejected_by_age(name: &str, version: &str, published: &str, offered: &str) -> String {
    format!(
        "hint: {name} {version} was published {published}, inside minimum-package-age, \
         so it is not offered until {offered}",
    )
}
