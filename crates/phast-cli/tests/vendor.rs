use phast_cli::installer::vendor;
use phast_core::lock::phast::{
    CURRENT_VERSION, DistSection, DistType, MetadataSection, PackageDef, PhastLock,
};

/// A lock holding `packages` and `packages_dev` and nothing else of interest.
fn lock(packages: Vec<PackageDef>, packages_dev: Vec<PackageDef>) -> PhastLock {
    PhastLock {
        version: CURRENT_VERSION,
        content_hash: "sha256:test".to_owned(),
        metadata: MetadataSection {
            generated_by: "phast 0.1.0".to_owned(),
            generated_at: "2026-06-14T00:00:00Z".to_owned(),
        },
        packages,
        packages_dev,
        autoload: Default::default(),
        autoload_dev: Default::default(),
    }
}

/// A registry package `name` at `version` requiring `require`, with no `dist`.
fn external(name: &str, version: &str, require: &[&str]) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: version.to_owned(),
        require: require
            .iter()
            .map(|d| (d.to_string(), "*".to_owned()))
            .collect(),
        ..Default::default()
    }
}

/// A package `name` carrying a zip `dist` whose checksum is `shasum`.
fn dist(name: &str, shasum: Option<&str>) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        dist: Some(DistSection {
            dist_type: DistType::Zip,
            url: "https://example.com/pkg.zip".to_owned(),
            reference: None,
            shasum: shasum.map(str::to_owned),
        }),
        ..Default::default()
    }
}

/// A workspace member `name`, which a `path` is what marks it as.
fn member(name: &str, require: &[&str], require_dev: &[&str]) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: "0.0.0".to_owned(),
        path: Some(format!("packages/{name}")),
        require: require
            .iter()
            .map(|d| (d.to_string(), "*".to_owned()))
            .collect(),
        require_dev: require_dev
            .iter()
            .map(|d| (d.to_string(), "*".to_owned()))
            .collect(),
        ..Default::default()
    }
}

/// The package names, in the order they were given.
fn names(packages: &[PackageDef]) -> Vec<&str> {
    packages.iter().map(|p| p.name.as_str()).collect()
}

// ---------------------------------------------------------------------------
// Which packages an install fetches
// ---------------------------------------------------------------------------

#[test]
fn install_full_includes_prod_and_dev() {
    let lock = lock(
        vec![external("vendor/foo", "1.0.0", &[])],
        vec![external("vendor/phpunit", "11.0.0", &[])],
    );
    // Production in lock order, then dev appended, so the plan reads as the lock
    // does rather than in whatever order the two sections were merged.
    let plan = vendor::plan_install(&lock, None, false);
    assert_eq!(names(&plan), vec!["vendor/foo", "vendor/phpunit"]);
}

#[test]
fn install_no_dev_excludes_dev() {
    let lock = lock(
        vec![external("vendor/foo", "1.0.0", &[])],
        vec![external("vendor/phpunit", "11.0.0", &[])],
    );
    let plan = vendor::plan_install(&lock, None, true);
    assert_eq!(names(&plan), vec!["vendor/foo"]);
}

#[test]
fn install_full_excludes_members() {
    let lock = lock(
        vec![
            external("vendor/foo", "1.0.0", &[]),
            member("acme/core", &[], &[]),
        ],
        vec![],
    );
    let plan = vendor::plan_install(&lock, None, false);
    assert_eq!(names(&plan), vec!["vendor/foo"]);
}

#[test]
fn install_member_scopes_to_closure() {
    let lock = lock(
        vec![
            member("acme/core", &["vendor/foo"], &[]),
            member("acme/other", &["vendor/bar"], &[]),
            external("vendor/foo", "1.0.0", &[]),
            external("vendor/bar", "2.0.0", &[]),
        ],
        vec![],
    );
    let plan = vendor::plan_install(&lock, Some("acme/core"), false);
    assert_eq!(names(&plan), vec!["vendor/foo"]);
}

#[test]
fn install_member_no_dev_excludes_the_members_dev_requirements() {
    let lock = lock(
        vec![
            member("acme/core", &["vendor/foo"], &["vendor/test"]),
            external("vendor/foo", "1.0.0", &[]),
            external("vendor/test", "1.0.0", &[]),
        ],
        vec![],
    );

    // A member's dev requirements sit in its own entry rather than in the lock's
    // dev section, so the flag has to reach the closure walk to have any effect.
    assert_eq!(
        names(&vendor::plan_install(&lock, Some("acme/core"), false)),
        vec!["vendor/foo", "vendor/test"],
    );
    assert_eq!(
        names(&vendor::plan_install(&lock, Some("acme/core"), true)),
        vec!["vendor/foo"],
    );
}

// ---------------------------------------------------------------------------
// Which packages an update has to re-download
// ---------------------------------------------------------------------------

#[test]
fn changed_detects_version_bump() {
    let old = lock(vec![external("vendor/foo", "1.0.0", &[])], vec![]);
    let new = lock(vec![external("vendor/foo", "1.1.0", &[])], vec![]);
    let changed = vendor::changed_versions(Some(&old), &new);
    assert!(changed.contains("vendor/foo"));
}

#[test]
fn changed_treats_new_package_as_changed() {
    let old = lock(vec![external("vendor/foo", "1.0.0", &[])], vec![]);
    let new = lock(
        vec![
            external("vendor/foo", "1.0.0", &[]),
            external("vendor/bar", "2.0.0", &[]),
        ],
        vec![],
    );
    let changed = vendor::changed_versions(Some(&old), &new);
    assert!(changed.contains("vendor/bar"));
    assert!(!changed.contains("vendor/foo"));
}

#[test]
fn changed_ignores_unchanged_package() {
    let old = lock(vec![external("vendor/foo", "1.0.0", &[])], vec![]);
    let new = lock(vec![external("vendor/foo", "1.0.0", &[])], vec![]);
    let changed = vendor::changed_versions(Some(&old), &new);
    assert!(!changed.contains("vendor/foo"));
}

#[test]
fn changed_detects_a_dev_version_bump() {
    let old = lock(vec![], vec![external("vendor/phpunit", "11.0.0", &[])]);
    let new = lock(vec![], vec![external("vendor/phpunit", "11.5.0", &[])]);

    // Both sections are compared, so a dev-only package is re-downloaded like
    // any other rather than being left at the version already on disk.
    let changed = vendor::changed_versions(Some(&old), &new);
    assert!(changed.contains("vendor/phpunit"));
}

#[test]
fn a_package_dropped_from_the_new_lock_is_not_changed() {
    let old = lock(
        vec![
            external("vendor/gone", "1.0.0", &[]),
            external("vendor/kept", "1.0.0", &[]),
        ],
        vec![],
    );
    let new = lock(vec![external("vendor/kept", "1.0.0", &[])], vec![]);

    // Only the new lock is walked. A removal is not something to re-download,
    // so it never reaches this set; clearing it out is the vendor sweep's job.
    let changed = vendor::changed_versions(Some(&old), &new);
    assert!(changed.is_empty(), "got: {changed:?}");
}

#[test]
fn changed_with_no_old_lock_marks_all() {
    let new = lock(
        vec![external("vendor/foo", "1.0.0", &[])],
        vec![external("vendor/phpunit", "11.0.0", &[])],
    );
    let changed = vendor::changed_versions(None, &new);
    assert!(changed.contains("vendor/foo"));
    assert!(changed.contains("vendor/phpunit"));
}

// ---------------------------------------------------------------------------
// Which packages arrive without a checksum to verify
// ---------------------------------------------------------------------------

#[test]
fn unverified_flags_missing_and_empty_shasum() {
    let packages = vec![
        dist("vendor/verified", Some("abc123")),
        dist("vendor/empty", Some("")),
        dist("vendor/missing", None),
        // No dist at all: never fetched, so never flagged as unverified.
        external("vendor/nodist", "1.0.0", &[]),
    ];
    let unverified = vendor::unverified_packages(&packages);
    assert_eq!(unverified, vec!["vendor/empty", "vendor/missing"]);
}
