//! `Workspace::scope`: which part of a project an invocation operates on.
//!
//! Two questions, asked together because the answer to one decides the other: which member
//! is narrowed to, and which directory that addresses. `relative` and `enclosing` are covered
//! in `workspace.rs`; what is covered here is the precedence between them.

use std::fs;
use std::path::PathBuf;

use phast_core::manifest::Manifest;
use phast_core::package::PackageName;
use phast_workspace::{Member, Workspace};

use anyhow::Result;
use indoc::indoc;
use tempfile::TempDir;

const WORKSPACE_ROOT_TOML: &str = indoc! {r#"
    [workspace]
    members = ["packages/*"]

    [workspace.dependencies]
    "vendor/shared" = "^3.0"

    [config]
    minimum-stability = "beta"

    [scripts]
    post-install-cmd = "echo root"
"#};

const API_TOML: &str = indoc! {r#"
    [package]
    name    = "acme/api"
    version = "1.0.0"

    [require]
    "vendor/alpha"  = "^1.0"
    "acme/logger"   = "*"
    "vendor/shared" = { workspace = true }

    [require-dev]
    "vendor/test-utils" = "^1.0"
"#};

const LOGGER_TOML: &str = indoc! {r#"
    [package]
    name    = "acme/logger"
    version = "1.0.0"

    [require]
    "vendor/beta" = "^2.0"
"#};

const STANDALONE_TOML: &str = indoc! {r#"
    [package]
    name    = "acme/standalone"
    version = "1.0.0"

    [require]
    "vendor/alpha" = "^1.0"
"#};

/// `[package].name` is optional, and a member without one is still a member.
const ANONYMOUS_TOML: &str = indoc! {r#"
    [package]
    version = "1.0.0"

    [require]
    "vendor/alpha" = "^1.0"
"#};

/// A project with no `[workspace]` section: one package, standing on its own.
fn standalone() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("phast.toml"), STANDALONE_TOML).unwrap();
    dir
}

/// A workspace root with two members, `acme/api` and `acme/logger`.
fn workspace() -> TempDir {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    fs::write(root.join("phast.toml"), WORKSPACE_ROOT_TOML).unwrap();
    fs::create_dir_all(root.join("packages/api")).unwrap();
    fs::create_dir_all(root.join("packages/logger")).unwrap();
    fs::write(root.join("packages/api/phast.toml"), API_TOML).unwrap();
    fs::write(root.join("packages/logger/phast.toml"), LOGGER_TOML).unwrap();

    dir
}

/// The same workspace, plus a `packages/anon` member declaring no name.
fn anonymous() -> TempDir {
    let dir = workspace();
    let anon = dir.path().join("packages/anon");

    fs::create_dir_all(&anon).unwrap();
    fs::write(anon.join("phast.toml"), ANONYMOUS_TOML).unwrap();

    dir
}

/// The directory a scope addresses for an invocation at the project root.
fn directory(dir: &TempDir, member: Option<&str>) -> Result<PathBuf> {
    let workspace = Workspace::resolve(dir.path())?;
    Ok(workspace.scope(dir.path(), member)?.dir().to_path_buf())
}

/// The member a scope narrows to, named, or `(none)` when nothing is narrowed.
fn narrowed(dir: &TempDir, cwd: &str, member: Option<&str>) -> String {
    let workspace = Workspace::resolve(dir.path()).unwrap();
    let workdir = dir.path().join(cwd);

    workspace
        .scope(&workdir, member)
        .unwrap()
        .member()
        .map_or_else(|| "(none)".to_owned(), |member| member.to_string())
}

/// The names a manifest requires, sorted, since `require` is a map.
fn requires(manifest: &Manifest) -> Vec<&str> {
    let mut names: Vec<&str> = manifest.require.keys().map(PackageName::as_str).collect();
    names.sort_unstable();
    names
}

// ---------------------------------------------------------------------------
// Which member is narrowed to
// ---------------------------------------------------------------------------

#[test]
fn member_at_a_workspace_root_is_none() {
    let dir = workspace();
    assert_eq!(narrowed(&dir, "", None), "(none)");
}

#[test]
fn member_in_a_standalone_project_is_none() {
    let dir = standalone();
    assert_eq!(narrowed(&dir, "", None), "(none)");
}

#[test]
fn member_named_by_the_flag() {
    let dir = workspace();
    assert_eq!(narrowed(&dir, "", Some("packages/api")), "acme/api");
}

#[test]
fn member_inferred_from_the_member_directory() {
    let dir = workspace();
    assert_eq!(narrowed(&dir, "packages/api", None), "acme/api");
}

#[test]
fn member_inferred_from_below_the_member_directory() {
    let dir = workspace();
    fs::create_dir_all(dir.path().join("packages/api/src")).unwrap();

    assert_eq!(narrowed(&dir, "packages/api/src", None), "acme/api");
}

#[test]
fn member_flag_overrides_the_directory() {
    let dir = workspace();
    assert_eq!(
        narrowed(&dir, "packages/api", Some("packages/logger")),
        "acme/logger",
    );
}

#[test]
fn member_flag_naming_nothing_errors() {
    let dir = workspace();
    let workspace = Workspace::resolve(dir.path()).unwrap();

    let result = workspace.scope(dir.path(), Some("packages/nope"));
    assert!(result.is_err());
}

#[test]
fn member_flag_outside_a_workspace_errors() {
    let dir = standalone();
    let workspace = Workspace::resolve(dir.path()).unwrap();

    let result = workspace.scope(dir.path(), Some("packages/api"));
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// A member with no declared name
// ---------------------------------------------------------------------------

#[test]
fn member_without_a_declared_name_still_resolves() {
    let dir = anonymous();
    let workspace = Workspace::resolve(dir.path()).unwrap();

    let scope = workspace.scope(dir.path(), Some("packages/anon")).unwrap();
    let member = scope.member().expect("an unnamed member is still a member");

    assert_eq!(member.name(), None);
    assert_eq!(member.path(), dir.path().join("packages/anon"));
}

#[test]
fn dir_resolves_a_member_without_a_declared_name() {
    let dir = anonymous();
    let result = directory(&dir, Some("packages/anon")).unwrap();

    assert_eq!(result, dir.path().join("packages/anon"));
}

// ---------------------------------------------------------------------------
// The directory it addresses
// ---------------------------------------------------------------------------

#[test]
fn dir_without_member_is_the_invocation_directory() {
    let dir = workspace();
    assert_eq!(directory(&dir, None).unwrap(), dir.path());
}

#[test]
fn dir_with_member_is_the_member_directory() {
    let dir = workspace();
    let result = directory(&dir, Some("packages/api")).unwrap();

    assert_eq!(result, dir.path().join("packages/api"));
}

#[test]
fn dir_with_unknown_member_errors() {
    let dir = workspace();
    assert!(directory(&dir, Some("packages/nonexistent")).is_err());
}

#[test]
fn dir_with_member_outside_a_workspace_errors() {
    let dir = standalone();
    assert!(directory(&dir, Some("packages/api")).is_err());
}

#[test]
fn dir_below_a_member_is_the_member_directory() {
    let dir = workspace();
    let src = dir.path().join("packages/api/src");
    fs::create_dir_all(&src).unwrap();

    let workspace = Workspace::resolve(dir.path()).unwrap();
    let result = workspace.scope(&src, None).unwrap().dir().to_path_buf();

    assert_eq!(result, dir.path().join("packages/api"));
}

// ---------------------------------------------------------------------------
// The root it points at
// ---------------------------------------------------------------------------

#[test]
fn root_is_the_project_root_when_nothing_is_narrowed() {
    let dir = workspace();
    let workspace = Workspace::resolve(dir.path()).unwrap();

    assert_eq!(
        workspace.scope(dir.path(), None).unwrap().root(),
        dir.path()
    );
}

#[test]
fn root_is_the_project_root_when_narrowed_by_the_flag() {
    let dir = workspace();
    let workspace = Workspace::resolve(dir.path()).unwrap();
    let scope = workspace.scope(dir.path(), Some("packages/api")).unwrap();

    assert_eq!(scope.root(), dir.path());
}

#[test]
fn root_is_the_project_root_from_inside_a_member() {
    let dir = workspace();
    let workspace = Workspace::resolve(dir.path()).unwrap();
    let scope = workspace
        .scope(&dir.path().join("packages/api"), None)
        .unwrap();

    assert_eq!(scope.root(), dir.path());
}

// ---------------------------------------------------------------------------
// The root as itself, rather than the view built from its members
// ---------------------------------------------------------------------------

#[test]
fn the_root_scope_holds_the_root_file_and_none_of_its_members() {
    let dir = workspace();
    let space = Workspace::resolve(dir.path()).unwrap();
    let scope = space.root_scope().unwrap();

    // What `validate` and `fmt` need: the file the author wrote. Its members'
    // requirements are somebody else's, so none of them appear here.
    assert!(requires(scope.manifest()).is_empty());
    assert!(scope.manifest().require_dev.is_empty());

    // The root's own sections do come back.
    assert!(scope.manifest().scripts.contains_key("post-install-cmd"));
    assert!(scope.member().is_none());
    assert_eq!(scope.dir(), dir.path());
}

#[test]
fn the_root_scope_of_a_standalone_project_is_its_own_package() {
    let dir = standalone();
    let space = Workspace::resolve(dir.path()).unwrap();
    let scope = space.root_scope().unwrap();

    // Standing alone is not a separate case: the root is the package, so its own
    // requirements are what the root file holds.
    assert_eq!(requires(scope.manifest()), ["vendor/alpha"]);
}

// ---------------------------------------------------------------------------
// What the scope requires
// ---------------------------------------------------------------------------

#[test]
fn an_unnarrowed_scope_folds_every_members_requirements_together() {
    let dir = workspace();
    let space = Workspace::resolve(dir.path()).unwrap();
    let scope = space.scope(dir.path(), None).unwrap();

    // What the resolver is handed: one requirement set for the whole project, with
    // the member that declared each one dropped, since a member is a source tree
    // and not something to fetch. The root's own [config] rides along.
    assert_eq!(
        requires(scope.manifest()),
        ["vendor/alpha", "vendor/beta", "vendor/shared"],
    );
    assert_eq!(
        scope.manifest().config.minimum_stability,
        phast_version::Stability::Beta,
    );
}

#[test]
fn a_narrowed_scope_requires_what_that_member_declared() {
    let dir = workspace();
    let space = Workspace::resolve(dir.path()).unwrap();
    let scope = space.scope(dir.path(), Some("packages/api")).unwrap();

    // Unlike the merged view, a member keeps its requirement on a sibling: that is
    // an edge it declared and can change in its own phast.toml.
    assert_eq!(
        requires(scope.manifest()),
        ["acme/logger", "vendor/alpha", "vendor/shared"],
    );
    assert_eq!(scope.manifest().name.as_deref(), Some("acme/api"));
}

#[test]
fn a_members_inherited_constraint_is_resolved_against_the_workspace() {
    let dir = workspace();
    let space = Workspace::resolve(dir.path()).unwrap();
    let scope = space.scope(dir.path(), Some("packages/api")).unwrap();

    // api writes `{ workspace = true }` for vendor/shared, so its manifest cannot
    // be read on its own: the constraint comes from [workspace.dependencies].
    let shared = scope
        .manifest()
        .require
        .get(&PackageName::new("vendor/shared"))
        .expect("the inherited requirement is present");
    assert_eq!(shared.to_string(), "^3.0");
}

// ---------------------------------------------------------------------------
// Every member of the project
// ---------------------------------------------------------------------------

#[test]
fn members_lists_the_whole_project_even_when_narrowed() {
    let dir = workspace();
    let space = Workspace::resolve(dir.path()).unwrap();

    // For a command that covers the project rather than narrowing to one part, so
    // it is the project's list and narrowing does not trim it.
    let scope = space.scope(dir.path(), Some("packages/api")).unwrap();
    let mut names: Vec<&str> = scope.members().iter().filter_map(Member::name).collect();
    names.sort_unstable();
    assert_eq!(names, ["acme/api", "acme/logger"]);
}

#[test]
fn members_is_empty_in_a_standalone_project() {
    let dir = standalone();
    let space = Workspace::resolve(dir.path()).unwrap();
    let scope = space.scope(dir.path(), None).unwrap();

    assert!(scope.members().is_empty());
}

// ---------------------------------------------------------------------------
// What falls within it
// ---------------------------------------------------------------------------

#[test]
fn includes_everything_when_nothing_is_narrowed() {
    let dir = workspace();
    let workspace = Workspace::resolve(dir.path()).unwrap();
    let scope = workspace.scope(dir.path(), None).unwrap();

    assert!(scope.includes("vendor/alpha", true));
    assert!(scope.includes("vendor/beta", true));
    assert!(scope.includes("vendor/anything", true));
}

#[test]
fn includes_a_direct_requirement_of_the_member() {
    let dir = workspace();
    let workspace = Workspace::resolve(dir.path()).unwrap();
    let scope = workspace.scope(dir.path(), Some("packages/api")).unwrap();

    assert!(scope.includes("vendor/alpha", true));
}

#[test]
fn includes_a_dev_requirement_when_dev_is_included() {
    let dir = workspace();
    let workspace = Workspace::resolve(dir.path()).unwrap();
    let scope = workspace.scope(dir.path(), Some("packages/api")).unwrap();

    assert!(scope.includes("vendor/test-utils", true));
}

#[test]
fn includes_a_sibling_the_member_requires_directly() {
    let dir = workspace();
    let space = Workspace::resolve(dir.path()).unwrap();
    let scope = space.scope(dir.path(), Some("packages/api")).unwrap();

    // A sibling is a direct requirement like any other: api declared it, so it is
    // api's to change and falls within its scope.
    assert!(scope.includes("acme/logger", true));
}

#[test]
fn the_dev_flag_is_ignored_when_nothing_is_narrowed() {
    let dir = workspace();
    let space = Workspace::resolve(dir.path()).unwrap();
    let scope = space.scope(dir.path(), None).unwrap();

    // Unnarrowed answers yes before it looks at the manifest, so the flag that
    // decides a member's answer cannot change this one.
    assert!(scope.includes("vendor/test-utils", false));
}

#[test]
fn excludes_a_package_only_a_sibling_requires() {
    let dir = workspace();
    let workspace = Workspace::resolve(dir.path()).unwrap();
    let scope = workspace.scope(dir.path(), Some("packages/api")).unwrap();

    assert!(!scope.includes("vendor/beta", true));
}

#[test]
fn excludes_a_dev_requirement_when_dev_is_excluded() {
    let dir = workspace();
    let workspace = Workspace::resolve(dir.path()).unwrap();
    let scope = workspace.scope(dir.path(), Some("packages/api")).unwrap();

    assert!(!scope.includes("vendor/test-utils", false));
}
