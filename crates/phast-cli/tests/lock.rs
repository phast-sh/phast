use phast_cli::project::lock;
use phast_core::lock::phast::{CURRENT_VERSION, MetadataSection, PackageDef, PhastLock};

/// A lock holding `packages` and `packages_dev` and nothing else of interest.
fn lock(packages: Vec<PackageDef>, packages_dev: Vec<PackageDef>) -> PhastLock {
    PhastLock {
        version: CURRENT_VERSION,
        content_hash: "sha256:test".to_owned(),
        metadata: MetadataSection {
            generated_by: "phast 0.1.0".to_owned(),
            generated_at: "2026-06-13T00:00:00Z".to_owned(),
        },
        packages,
        packages_dev,
        autoload: Default::default(),
        autoload_dev: Default::default(),
    }
}

/// A registry package `name` at `version`, carrying no `path`.
fn external(name: &str, version: &str) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: version.to_owned(),
        ..Default::default()
    }
}

/// A workspace member `name`, which a `path` is what marks it as.
fn member(name: &str) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: "0.0.0".to_owned(),
        path: Some(format!("packages/{name}")),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Collecting the packages an install has to fetch
// ---------------------------------------------------------------------------

#[test]
fn collect_returns_production_packages() {
    let lock = lock(
        vec![
            external("vendor/foo", "1.0.0"),
            external("vendor/bar", "2.0.0"),
        ],
        vec![],
    );
    let result = lock::collect(&lock, false);
    assert_eq!(result.len(), 2);
    assert!(result.contains(&("vendor/foo".to_owned(), "1.0.0".to_owned())));
    assert!(result.contains(&("vendor/bar".to_owned(), "2.0.0".to_owned())));
}

#[test]
fn collect_includes_dev_packages_with_include_dev() {
    let lock = lock(
        vec![external("vendor/foo", "1.0.0")],
        vec![external("vendor/phpunit", "11.0.0")],
    );
    let result = lock::collect(&lock, true);
    assert_eq!(result.len(), 2);
}

#[test]
fn collect_excludes_dev_packages_without_include_dev() {
    let lock = lock(
        vec![external("vendor/foo", "1.0.0")],
        vec![external("vendor/phpunit", "11.0.0")],
    );
    let result = lock::collect(&lock, false);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "vendor/foo");
}

#[test]
fn collect_excludes_member_entries_from_prod() {
    let lock = lock(
        vec![external("vendor/foo", "1.0.0"), member("acme/core")],
        vec![],
    );
    let result = lock::collect(&lock, false);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "vendor/foo");
}

#[test]
fn collect_excludes_member_entries_from_dev() {
    let lock = lock(
        vec![],
        vec![
            external("vendor/phpunit", "11.0.0"),
            member("acme/test-helpers"),
        ],
    );
    let result = lock::collect(&lock, true);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "vendor/phpunit");
}

#[test]
fn collect_keeps_production_first_then_dev() {
    let lock = lock(
        vec![external("a/one", "1.0.0"), external("b/two", "2.0.0")],
        vec![external("z/dev", "9.0.0")],
    );

    // Dev entries are appended rather than merged, so the lock's own order,
    // which puts a dependency before its dependents, survives.
    let names: Vec<String> = lock::collect(&lock, true)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, vec!["a/one", "b/two", "z/dev"]);
}

#[test]
fn collect_empty_lock_returns_empty() {
    let lock = lock(vec![], vec![]);
    assert!(lock::collect(&lock, true).is_empty());
}

#[test]
fn collect_only_members_returns_empty() {
    let lock = lock(vec![member("acme/core"), member("acme/logger")], vec![]);
    assert!(lock::collect(&lock, true).is_empty());
}

// ---------------------------------------------------------------------------
// Finding one package by name
// ---------------------------------------------------------------------------

#[test]
fn find_locates_package_by_name() {
    let lock = lock(
        vec![external("vendor/foo", "1.2.3")],
        vec![external("vendor/bar", "4.5.6")],
    );
    let found = lock::find(&lock, "vendor/foo").unwrap();
    assert_eq!(found.version, "1.2.3");
}

#[test]
fn find_searches_dev_packages() {
    let lock = lock(vec![], vec![external("vendor/phpunit", "11.0.0")]);
    assert!(lock::find(&lock, "vendor/phpunit").is_some());
}

#[test]
fn find_returns_none_for_missing_package() {
    let lock = lock(vec![external("vendor/foo", "1.0.0")], vec![]);
    assert!(lock::find(&lock, "vendor/missing").is_none());
}

#[test]
fn find_is_case_insensitive() {
    let lock = lock(
        vec![
            external("Vendor/Foo", "1.0.0"),
            external("psr/log", "3.0.0"),
        ],
        vec![],
    );

    // Both directions, because a user typing `phast show PSR/Log` is the case
    // that motivates this and it is the one the stored name is lowercase in.
    assert!(lock::find(&lock, "vendor/foo").is_some());
    assert!(lock::find(&lock, "PSR/Log").is_some());
}

#[test]
fn find_reaches_a_workspace_member() {
    let lock = lock(
        vec![external("vendor/foo", "1.0.0"), member("acme/core")],
        vec![],
    );

    // The two answer different questions about the same lock: `collect` names
    // what has to be fetched, so it leaves members out, while `find` answers
    // what is installed, which a member is.
    assert!(lock::find(&lock, "acme/core").is_some());
    assert_eq!(lock::collect(&lock, true).len(), 1);
}

#[test]
fn find_prefers_a_production_entry_over_a_dev_one() {
    let lock = lock(
        vec![external("dup/pkg", "1.0.0")],
        vec![external("dup/pkg", "9.9.9")],
    );

    // Production is searched first, so a name in both sections resolves there.
    assert_eq!(lock::find(&lock, "dup/pkg").unwrap().version, "1.0.0");
}
