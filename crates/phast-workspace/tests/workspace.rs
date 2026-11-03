use std::fs;
use std::path::Path;

use phast_core::package::PackageName;
use phast_workspace::{Workspace, find_root};

use indoc::{formatdoc, indoc};
use tempfile::TempDir;

const WORKSPACE_ROOT_TOML: &str = indoc! {r#"
    [workspace]
    members = ["packages/*"]
"#};

const API_TOML: &str = indoc! {r#"
    [package]
    name    = "acme/api"
    version = "1.0.0"

    [require]
    "vendor/alpha" = "^1.0"
"#};

const LOGGER_TOML: &str = indoc! {r#"
    [package]
    name    = "acme/logger"
    version = "1.0.0"
"#};

const STANDALONE_TOML: &str = indoc! {r#"
    [package]
    name = "acme/app"
"#};

/// A member declaring a name but no version, which only matters once something
/// requires it.
const VERSIONLESS_TOML: &str = indoc! {r#"
    [package]
    name = "acme/api"
"#};

/// `[package].name` is optional, and a member without one has no lock identity.
const NAMELESS_TOML: &str = indoc! {r#"
    [package]
    version = "1.0.0"
"#};

/// A project tree holding each `(relative path, contents)` file, with the
/// directories above it created on the way.
fn tree(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    for (path, contents) in files {
        let file = dir.path().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, contents).unwrap();
    }
    dir
}

/// A workspace root with two members, `acme/api` and `acme/logger`.
fn workspace() -> TempDir {
    tree(&[
        ("phast.toml", WORKSPACE_ROOT_TOML),
        ("packages/api/phast.toml", API_TOML),
        ("packages/logger/phast.toml", LOGGER_TOML),
    ])
}

/// A project with no `[workspace]` section: one package, standing on its own.
fn standalone() -> TempDir {
    tree(&[("phast.toml", STANDALONE_TOML)])
}

/// A two-member workspace where `acme/api` requires `acme/logger` under
/// `constraint` in `section`, and logger declares `version` (`None` for no version).
fn siblings(constraint: &str, version: Option<&str>, section: &str) -> TempDir {
    let api = formatdoc! {r#"
        [package]
        name    = "acme/api"
        version = "1.0.0"

        [{section}]
        "acme/logger" = "{constraint}"
    "#};
    let logger = match version {
        Some(version) => formatdoc! {r#"
            [package]
            name    = "acme/logger"
            version = "{version}"
        "#},
        None => indoc! {r#"
            [package]
            name = "acme/logger"
        "#}
        .to_owned(),
    };

    tree(&[
        ("phast.toml", WORKSPACE_ROOT_TOML),
        ("packages/api/phast.toml", &api),
        ("packages/logger/phast.toml", &logger),
    ])
}

/// Why loading the workspace at `dir` failed, or a panic if it did not.
fn refusal(dir: &TempDir) -> String {
    match Workspace::load(dir.path()) {
        Ok(_) => panic!("expected the root to be refused"),
        Err(error) => error.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Finding the root of a project
// ---------------------------------------------------------------------------

#[test]
fn a_workspace_root_finds_itself() {
    let dir = workspace();
    assert_eq!(find_root(dir.path()), Some(dir.path().to_path_buf()));
}

#[test]
fn the_search_walks_up_from_a_member_directory() {
    let dir = workspace();
    let member = dir.path().join("packages/api");
    assert_eq!(find_root(&member), Some(dir.path().to_path_buf()));
}

#[test]
fn a_standalone_project_has_no_root_to_find() {
    // A `phast.toml` with no `[workspace]` section is a package, not a root.
    let dir = standalone();
    assert_eq!(find_root(dir.path()), None);
}

#[test]
fn a_directory_with_no_manifest_has_no_root_to_find() {
    let dir = TempDir::new().unwrap();
    assert_eq!(find_root(dir.path()), None);
}

// ---------------------------------------------------------------------------
// Loading a root, and what it refuses
// ---------------------------------------------------------------------------

#[test]
fn an_ordinary_project_directory_is_not_a_root() {
    // Checked before the shape guards below, since telling an author how a
    // workspace root must be written is the wrong advice if they meant a package.
    let error = refusal(&standalone());
    assert!(error.contains("is not a workspace root"), "{error}");
    assert!(error.contains("no [workspace] section"), "{error}");
}

#[test]
fn a_root_declaring_a_package_is_refused() {
    let dir = tree(&[
        (
            "phast.toml",
            indoc! {r#"
                [workspace]
                members = ["packages/*"]

                [package]
                name = "acme/root"
            "#},
        ),
        ("packages/api/phast.toml", API_TOML),
    ]);

    // A root declares members, not a package of its own.
    let error = refusal(&dir);
    assert!(error.contains("must not declare a [package]"), "{error}");
}

#[test]
fn a_root_declaring_requirements_is_refused() {
    // Shared versions go in [workspace.dependencies], which members opt into, so a
    // root cannot quietly add a requirement every member inherits.
    for section in ["require", "require-dev"] {
        let root = formatdoc! {r#"
            [workspace]
            members = ["packages/*"]

            [{section}]
            "vendor/alpha" = "^1.0"
        "#};
        let dir = tree(&[("phast.toml", &root), ("packages/api/phast.toml", API_TOML)]);

        let error = refusal(&dir);
        assert!(error.contains(section), "{error}");
        assert!(error.contains("[workspace.dependencies]"), "{error}");
    }
}

#[test]
fn a_root_declaring_no_members_is_refused() {
    let dir = tree(&[("phast.toml", "[workspace]\nmembers = []\n")]);

    // An active workspace always has something to iterate, which is what lets
    // `is_active` be answered by the member list alone.
    let error = refusal(&dir);
    assert!(error.contains("members is empty"), "{error}");
}

#[test]
fn patterns_that_match_no_member_are_refused() {
    let dir = tree(&[
        ("phast.toml", "[workspace]\nmembers = [\"nowhere/*\"]\n"),
        ("packages/api/phast.toml", API_TOML),
    ]);

    // Distinct from declaring none: the author named a pattern and it found
    // nothing, so the message quotes the pattern back.
    let error = refusal(&dir);
    assert!(error.contains("nowhere/*"), "{error}");
}

#[test]
fn a_require_cycle_between_members_is_refused_at_load() {
    let dir = tree(&[
        ("phast.toml", WORKSPACE_ROOT_TOML),
        (
            "packages/a/phast.toml",
            indoc! {r#"
                [package]
                name    = "acme/a"
                version = "1.0.0"

                [require]
                "acme/b" = "^1.0"
            "#},
        ),
        (
            "packages/b/phast.toml",
            indoc! {r#"
                [package]
                name    = "acme/b"
                version = "1.0.0"

                [require]
                "acme/a" = "^1.0"
            "#},
        ),
    ]);

    // The graph is checked up front rather than letting the merge silently strip
    // the offending edge.
    let error = refusal(&dir);
    assert!(error.contains("require cycle"), "{error}");
}

// ---------------------------------------------------------------------------
// Whether workspace mode is active
// ---------------------------------------------------------------------------

#[test]
fn a_loaded_workspace_is_active() {
    let dir = workspace();
    let space = Workspace::load(dir.path()).unwrap();
    assert!(space.is_active());
}

#[test]
fn a_standalone_project_is_never_active() {
    let dir = standalone();
    let space = Workspace::resolve(dir.path()).unwrap();

    // Having members is the whole test, and a standalone project has none.
    assert!(!space.is_active());
    assert!(space.members().is_empty());
    assert_eq!(space.root(), dir.path());
}

// ---------------------------------------------------------------------------
// Looking a member up
// ---------------------------------------------------------------------------

#[test]
fn a_member_is_found_by_its_absolute_path() {
    let dir = workspace();
    let space = Workspace::load(dir.path()).unwrap();
    assert!(space.absolute(&dir.path().join("packages/api")).is_some());
}

#[test]
fn a_member_is_found_by_its_path_relative_to_the_root() {
    let dir = workspace();
    let space = Workspace::load(dir.path()).unwrap();
    assert!(space.relative(Path::new("packages/api")).is_some());
}

#[test]
fn the_two_lookups_do_not_accept_each_others_paths() {
    let dir = workspace();
    let space = Workspace::load(dir.path()).unwrap();

    // Each compares against the member's stored absolute path, so a relative path
    // handed to `absolute` matches nothing rather than being joined to the root.
    assert!(space.absolute(Path::new("packages/api")).is_none());
}

#[test]
fn a_path_no_member_lives_at_is_not_found() {
    let dir = workspace();
    let space = Workspace::load(dir.path()).unwrap();
    assert!(space.absolute(Path::new("/nonexistent/path")).is_none());
    assert!(space.relative(Path::new("packages/nonexistent")).is_none());
}

#[test]
fn a_member_directory_encloses_itself() {
    let dir = workspace();
    let space = Workspace::load(dir.path()).unwrap();
    let member = dir.path().join("packages/api");

    assert_eq!(space.enclosing(&member).unwrap().path(), member);
}

#[test]
fn a_directory_inside_a_member_is_enclosed_by_it() {
    let dir = workspace();
    let space = Workspace::load(dir.path()).unwrap();
    let nested = dir.path().join("packages/api/src");
    fs::create_dir_all(&nested).unwrap();

    assert!(space.enclosing(&nested).is_some());
}

#[test]
fn the_workspace_root_is_enclosed_by_no_member() {
    let dir = workspace();
    let space = Workspace::load(dir.path()).unwrap();
    assert!(space.enclosing(dir.path()).is_none());
}

#[test]
fn a_path_outside_the_workspace_is_enclosed_by_no_member() {
    let dir = workspace();
    let space = Workspace::load(dir.path()).unwrap();
    assert!(space.enclosing(Path::new("/tmp")).is_none());
}

// ---------------------------------------------------------------------------
// A member requiring a sibling
// ---------------------------------------------------------------------------

#[test]
fn a_satisfied_sibling_constraint_loads() {
    let dir = siblings("^1.0", Some("1.2.0"), "require");
    assert!(Workspace::load(dir.path()).is_ok());
}

#[test]
fn a_sibling_constraint_the_declared_version_misses_is_refused() {
    let dir = siblings("^2.0", Some("1.0.0"), "require");

    // All four parts, because the author needs to know which member, which
    // constraint, and what the sibling actually declares.
    let error = refusal(&dir);
    assert!(error.contains("acme/api"), "missing requirer: {error}");
    assert!(error.contains("acme/logger"), "missing member: {error}");
    assert!(error.contains("^2.0"), "missing constraint: {error}");
    assert!(error.contains("1.0.0"), "missing declared version: {error}");
}

#[test]
fn a_dev_requirement_on_a_sibling_is_checked_the_same_way() {
    let dir = siblings("^2.0", Some("1.0.0"), "require-dev");
    assert!(Workspace::load(dir.path()).is_err());
}

#[test]
fn a_sibling_with_no_version_is_refused_when_required() {
    let dir = siblings("^1.0", None, "require");
    let error = refusal(&dir);
    assert!(error.contains("declares no version"), "{error}");
}

#[test]
fn a_member_with_no_version_loads_when_nobody_requires_it() {
    // A version is only needed to check somebody's constraint against.
    let dir = tree(&[
        ("phast.toml", WORKSPACE_ROOT_TOML),
        ("packages/api/phast.toml", VERSIONLESS_TOML),
    ]);

    assert!(Workspace::load(dir.path()).is_ok());
}

#[test]
fn a_satisfied_sibling_requirement_is_stripped_from_the_merged_manifest() {
    let dir = siblings("^1.0", Some("1.0.0"), "require");
    let space = Workspace::load(dir.path()).unwrap();

    // The merged view is for the resolver, and a member is a source tree rather
    // than something to fetch.
    assert!(
        !space
            .manifest()
            .require
            .contains_key(&PackageName::new("acme/logger")),
        "sibling dep leaked into the merged manifest",
    );
}

// ---------------------------------------------------------------------------
// The shared constraints the root owns
// ---------------------------------------------------------------------------

#[test]
fn the_root_keeps_its_shared_constraints_after_expansion() {
    let dir = tree(&[
        (
            "phast.toml",
            indoc! {r#"
                [workspace]
                members = ["packages/*"]

                [workspace.dependencies]
                "vendor/shared" = "^3.0"
            "#},
        ),
        ("packages/api/phast.toml", API_TOML),
    ]);
    let space = Workspace::load(dir.path()).unwrap();

    // Kept separate from the merged manifest, which records what members ended up
    // requiring rather than which constraints the root owns.
    assert_eq!(space.dependencies().get("vendor/shared"), Some("^3.0"));
}

#[test]
fn a_standalone_project_owns_no_shared_constraints() {
    let dir = standalone();
    let space = Workspace::resolve(dir.path()).unwrap();
    assert_eq!(space.dependencies().get("vendor/shared"), None);
}

// ---------------------------------------------------------------------------
// Resolving without knowing which mode the project is in
// ---------------------------------------------------------------------------

#[test]
fn find_reports_a_workspace_from_the_root_or_from_inside_a_member() {
    let dir = workspace();
    assert!(Workspace::find(dir.path()).unwrap().is_some());
    let member = dir.path().join("packages/api");
    assert!(Workspace::find(&member).unwrap().is_some());
}

#[test]
fn find_reports_no_workspace_for_a_standalone_project() {
    // Not an error: the caller decides what to do with a project standing alone.
    let dir = standalone();
    assert!(Workspace::find(dir.path()).unwrap().is_none());
}

#[test]
fn find_reports_an_error_for_a_root_it_cannot_use() {
    // The third outcome, distinct from the other two: a root was found and is
    // broken, which must not be mistaken for there being none.
    let dir = tree(&[("phast.toml", "[workspace]\nmembers = []\n")]);
    assert!(Workspace::find(dir.path()).is_err());
}

// ---------------------------------------------------------------------------
// The lock entries describing members
// ---------------------------------------------------------------------------

#[test]
fn each_named_member_becomes_a_lock_entry_sorted_by_name() {
    let dir = tree(&[
        ("phast.toml", WORKSPACE_ROOT_TOML),
        (
            "packages/api/phast.toml",
            indoc! {r#"
                [package]
                name    = "acme/api"
                version = "2.5.0"

                [require]
                "vendor/alpha" = "^1.0"
                "acme/logger"  = "^1.0"

                [require-dev]
                "vendor/test" = "^3.0"
            "#},
        ),
        ("packages/logger/phast.toml", LOGGER_TOML),
    ]);
    let space = Workspace::load(dir.path()).unwrap();
    let entries = space.lock();

    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["acme/api", "acme/logger"]);

    // A `path` relative to the root is what marks the entry as a member, and the
    // requirements are recorded as written, intra-workspace edges included, so the
    // lock is enough to compute a per-member closure later.
    let api = &entries[0];
    assert_eq!(api.version, "2.5.0");
    assert_eq!(api.path.as_deref(), Some("packages/api"));
    assert_eq!(
        api.require.entries.get("acme/logger").map(String::as_str),
        Some("^1.0"),
    );
    let dev = &api.require_dev.entries;
    assert_eq!(dev.get("vendor/test").map(String::as_str), Some("^3.0"));
}

#[test]
fn a_member_declaring_no_name_gets_no_lock_entry() {
    let dir = tree(&[
        ("phast.toml", WORKSPACE_ROOT_TOML),
        ("packages/api/phast.toml", API_TOML),
        ("packages/anon/phast.toml", NAMELESS_TOML),
    ]);
    let space = Workspace::load(dir.path()).unwrap();

    // It has no package identity, so there is nothing to record it under.
    assert_eq!(space.members().len(), 2);
    assert_eq!(space.lock().len(), 1);
}

#[test]
fn a_member_with_no_version_is_locked_at_zero() {
    let dir = tree(&[
        ("phast.toml", WORKSPACE_ROOT_TOML),
        ("packages/api/phast.toml", VERSIONLESS_TOML),
    ]);
    let space = Workspace::load(dir.path()).unwrap();

    assert_eq!(space.lock()[0].version, "0.0.0");
}

#[test]
fn a_standalone_project_contributes_no_lock_entries() {
    let dir = standalone();
    let space = Workspace::resolve(dir.path()).unwrap();
    assert!(space.lock().is_empty());
}
