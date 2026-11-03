use phast_cli::commands::why::checker::{Source, TreeNode, WhyChecker};
use phast_core::lock::phast::PhastLock;
use phast_workspace::Workspace;

use indoc::{formatdoc, indoc};
use tempfile::TempDir;

/// A lock recording no packages; every test appends the ones it needs.
const LOCK_TOML: &str = indoc! {r#"
    version      = 1
    content-hash = "sha256:test"

    [metadata]
    generated-by = "phast 0.1.0"
    generated-at = "2026-01-01T00:00:00+00:00"
"#};

/// A root requiring nothing, so every requirer found came from the lock.
const ROOT_TOML: &str = indoc! {r#"
    [package]
    name = "test/project"
"#};

const REQUIRES_ALPHA_TOML: &str = indoc! {r#"
    [package]
    name = "test/project"

    [require]
    "vendor/alpha" = "^1.0"
"#};

const REQUIRES_BETA_TOML: &str = indoc! {r#"
    [package]
    name = "test/project"

    [require]
    "vendor/beta" = "^2.0"
"#};

const REQUIRES_BETA_DEV_TOML: &str = indoc! {r#"
    [package]
    name = "test/project"

    [require-dev]
    "vendor/beta" = "^2.0"
"#};

/// The empty lock with `packages` appended, so a fixture declares only its own
/// entries.
#[rustfmt::skip]
fn lock(packages: &str) -> PhastLock {
    toml::from_str(&format!("\
        {LOCK_TOML}\n\
        {packages}\
    ")).expect("invalid lock TOML")
}

/// A workspace over `toml`, with the directory it was written to: dropping that would
/// delete the file the manifest was read from, and a scope borrows from the workspace.
///
/// Standalone, so `members()` is empty and every requirement is the root's own.
fn workspace(toml: &str) -> (Workspace, TempDir) {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("phast.toml"), toml).unwrap();

    let workspace = Workspace::resolve(dir.path()).unwrap();
    (workspace, dir)
}

/// A workspace whose members each require one package, given as `(name, requires)`.
///
/// Members live at `packages/<name>` and are declared as `acme/<name>`, so the path
/// a test narrows with and the name a requirer reports are deliberately different.
fn monorepo(members: &[(&str, &str)]) -> (Workspace, TempDir) {
    let dir = TempDir::new().unwrap();
    let manifest = indoc! {r#"
        [workspace]
        members = ["packages/*"]
    "#};
    std::fs::write(dir.path().join("phast.toml"), manifest).unwrap();

    for (name, requires) in members {
        let member = dir.path().join(format!("packages/{name}"));
        std::fs::create_dir_all(&member).unwrap();
        let manifest = formatdoc! {r#"
            [package]
            name    = "acme/{name}"
            version = "1.0.0"

            [require]
            "{requires}" = "^1.0"
        "#};
        std::fs::write(member.join("phast.toml"), manifest).unwrap();
    }

    let workspace = Workspace::resolve(dir.path()).unwrap();
    (workspace, dir)
}

// ---------------------------------------------------------------------------
// Flat mode: direct requirers
// ---------------------------------------------------------------------------

#[test]
fn direct_requirer_from_locked_package() {
    let lock = lock(indoc! {r#"
        [[packages]]
        name    = "vendor/alpha"
        version = "1.0.0"
        require = { "vendor/beta" = "^2.0" }

        [[packages]]
        name    = "vendor/beta"
        version = "2.0.0"
    "#});
    let (workspace, dir) = workspace(ROOT_TOML);
    let scope = workspace.scope(dir.path(), None).unwrap();

    let result = WhyChecker::new(&scope, &lock, true)
        .unwrap()
        .check("vendor/beta");

    assert_eq!(result.requirers.len(), 1);
    assert!(matches!(&result.requirers[0].source, Source::Package(name) if name == "vendor/alpha"));
    assert_eq!(result.requirers[0].constraint, "^2.0");
    assert!(!result.requirers[0].dev);
}

#[test]
fn multiple_requirers_from_lock() {
    let lock = lock(indoc! {r#"
        [[packages]]
        name    = "vendor/alpha"
        version = "1.0.0"
        require = { "vendor/shared" = "^1.0" }

        [[packages]]
        name    = "vendor/gamma"
        version = "3.0.0"
        require = { "vendor/shared" = "^1.0" }

        [[packages]]
        name    = "vendor/shared"
        version = "1.2.0"
    "#});
    let (workspace, dir) = workspace(ROOT_TOML);
    let scope = workspace.scope(dir.path(), None).unwrap();

    let result = WhyChecker::new(&scope, &lock, true)
        .unwrap()
        .check("vendor/shared");

    let names: Vec<&str> = result.requirers.iter().map(|r| r.as_str()).collect();
    assert_eq!(result.requirers.len(), 2);
    assert!(names.contains(&"vendor/alpha"));
    assert!(names.contains(&"vendor/gamma"));
}

#[test]
fn root_manifest_shown_as_requirer() {
    let lock = lock(indoc! {r#"
        [[packages]]
        name    = "vendor/beta"
        version = "2.0.0"
    "#});
    let (workspace, dir) = workspace(REQUIRES_BETA_TOML);
    let scope = workspace.scope(dir.path(), None).unwrap();

    let result = WhyChecker::new(&scope, &lock, true)
        .unwrap()
        .check("vendor/beta");

    assert_eq!(result.requirers.len(), 1);
    assert!(matches!(result.requirers[0].source, Source::Root));
    assert!(!result.requirers[0].dev);
}

#[test]
fn dev_root_requirement_included_when_include_dev() {
    let lock = lock(indoc! {r#"
        [[packages-dev]]
        name    = "vendor/beta"
        version = "2.0.0"
    "#});
    let (workspace, dir) = workspace(REQUIRES_BETA_DEV_TOML);
    let scope = workspace.scope(dir.path(), None).unwrap();

    let result = WhyChecker::new(&scope, &lock, true)
        .unwrap()
        .check("vendor/beta");

    assert_eq!(result.requirers.len(), 1);
    assert!(matches!(result.requirers[0].source, Source::Root));
    assert!(result.requirers[0].dev);
}

#[test]
fn dev_root_requirement_excluded_when_no_dev() {
    let lock = lock(indoc! {r#"
        [[packages-dev]]
        name    = "vendor/beta"
        version = "2.0.0"
    "#});
    let (workspace, dir) = workspace(REQUIRES_BETA_DEV_TOML);
    let scope = workspace.scope(dir.path(), None).unwrap();

    let result = WhyChecker::new(&scope, &lock, false)
        .unwrap()
        .check("vendor/beta");

    assert!(result.requirers.is_empty());
}

#[test]
fn empty_requirers_for_unreferenced_package() {
    let lock = lock(indoc! {r#"
        [[packages]]
        name    = "vendor/orphan"
        version = "1.0.0"
    "#});
    let (workspace, dir) = workspace(ROOT_TOML);
    let scope = workspace.scope(dir.path(), None).unwrap();

    let result = WhyChecker::new(&scope, &lock, true)
        .unwrap()
        .check("vendor/orphan");

    assert!(result.requirers.is_empty());
}

// ---------------------------------------------------------------------------
// Flat mode: who a requirer is in a workspace
// ---------------------------------------------------------------------------

#[test]
fn every_member_that_requires_a_package_is_named() {
    let lock = lock("");
    let (workspace, dir) = monorepo(&[("api", "vendor/shared"), ("web", "vendor/shared")]);
    let scope = workspace.scope(dir.path(), None).unwrap();

    let result = WhyChecker::new(&scope, &lock, true)
        .unwrap()
        .check("vendor/shared");

    // Each member is read on its own, so a requirement keeps the name of whoever
    // wrote it; the merged manifest a workspace resolves against has lost that.
    let sources: Vec<&str> = result
        .requirers
        .iter()
        .map(|r| match &r.source {
            Source::Member(name) => name.as_str(),
            Source::Root => panic!("a workspace root requires nothing of its own"),
            Source::Package(name) => panic!("unexpected package requirer {name}"),
        })
        .collect();
    assert_eq!(sources, vec!["acme/api", "acme/web"]);
}

#[test]
fn narrowing_to_a_member_leaves_out_what_a_sibling_requires() {
    let lock = lock("");
    let (workspace, dir) = monorepo(&[("api", "vendor/shared"), ("web", "vendor/shared")]);
    let scope = workspace.scope(dir.path(), Some("packages/api")).unwrap();

    let result = WhyChecker::new(&scope, &lock, true)
        .unwrap()
        .check("vendor/shared");

    // Narrowed by path, reported by package name, and `acme/web` wants the same
    // package but is not part of the graph the question was asked about.
    assert_eq!(result.requirers.len(), 1);
    assert!(matches!(&result.requirers[0].source, Source::Member(name) if name == "acme/api"));
}

// ---------------------------------------------------------------------------
// Tree mode: nodes and cycle detection
// ---------------------------------------------------------------------------

#[test]
fn tree_terminates_at_root() {
    let lock = lock(indoc! {r#"
        [[packages]]
        name    = "vendor/beta"
        version = "2.0.0"
    "#});
    let (workspace, dir) = workspace(REQUIRES_BETA_TOML);
    let scope = workspace.scope(dir.path(), None).unwrap();

    let result = WhyChecker::new(&scope, &lock, true)
        .unwrap()
        .check("vendor/beta");

    assert_eq!(result.nodes.len(), 1);
    assert!(matches!(result.nodes[0], TreeNode::Root { dev: false }));
}

#[test]
fn tree_follows_transitive_chain() {
    // root → alpha → beta → gamma
    let lock = lock(indoc! {r#"
        [[packages]]
        name    = "vendor/alpha"
        version = "1.0.0"
        require = { "vendor/beta" = "^2.0" }

        [[packages]]
        name    = "vendor/beta"
        version = "2.0.0"
        require = { "vendor/gamma" = "^3.0" }

        [[packages]]
        name    = "vendor/gamma"
        version = "3.0.0"
    "#});
    let (workspace, dir) = workspace(REQUIRES_ALPHA_TOML);
    let scope = workspace.scope(dir.path(), None).unwrap();

    let result = WhyChecker::new(&scope, &lock, true)
        .unwrap()
        .check("vendor/gamma");

    // Top-level node: vendor/beta requires gamma
    assert_eq!(result.nodes.len(), 1);
    let TreeNode::Package { name, children, .. } = &result.nodes[0] else {
        panic!("expected Package node");
    };
    assert_eq!(name, "vendor/beta");

    // Child: vendor/alpha requires beta
    assert_eq!(children.len(), 1);
    let TreeNode::Package {
        name: child_name,
        children: grandchildren,
        ..
    } = &children[0]
    else {
        panic!("expected Package node");
    };
    assert_eq!(child_name, "vendor/alpha");

    // Grandchild: root requires alpha
    assert_eq!(grandchildren.len(), 1);
    assert!(matches!(grandchildren[0], TreeNode::Root { dev: false }));
}

#[test]
fn tree_detects_cycle() {
    // alpha requires beta, beta requires alpha
    let lock = lock(indoc! {r#"
        [[packages]]
        name    = "vendor/alpha"
        version = "1.0.0"
        require = { "vendor/beta" = "^2.0" }

        [[packages]]
        name    = "vendor/beta"
        version = "2.0.0"
        require = { "vendor/alpha" = "^1.0" }
    "#});
    let (workspace, dir) = workspace(REQUIRES_ALPHA_TOML);
    let scope = workspace.scope(dir.path(), None).unwrap();

    let result = WhyChecker::new(&scope, &lock, true)
        .unwrap()
        .check("vendor/beta");

    // Top node: alpha requires beta
    assert_eq!(result.nodes.len(), 1);
    let TreeNode::Package { children, .. } = &result.nodes[0] else {
        panic!("expected Package node");
    };

    // At least one child should be a Circular sentinel (beta → alpha → beta loops back)
    let has_circular = children.iter().any(|c| matches!(c, TreeNode::Circular));
    assert!(has_circular, "expected a Circular sentinel in children");
}
