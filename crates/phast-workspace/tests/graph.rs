use std::path::PathBuf;

use phast_core::lock::phast::{CURRENT_VERSION, MetadataSection, PackageDef, PhastLock};
use phast_core::manifest::phast::{DependencySpec, PackageSection, PhastManifest, RequireSection};
use phast_workspace::Member;
use phast_workspace::graph::{closure, toposort};

use indexmap::IndexMap;

/// A resolved lock holding `packages` as its production entries and `packages_dev`
/// as the dev-only ones. `closure` reads both, so which list an entry sits in only
/// decides whether a dev-free walk reaches it.
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

/// A locked registry package at 1.0.0 requiring `require`. No `path`, which is what
/// marks it installable rather than a source tree.
fn external(name: &str, require: &[&str]) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        require: dependencies(require),
        ..Default::default()
    }
}

/// A locked workspace member. The `path` is what `closure` reads to know it is a
/// source tree, so it is traversed but never returned.
fn member(name: &str, require: &[&str], require_dev: &[&str]) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: "0.0.0".to_owned(),
        path: Some(format!("packages/{name}")),
        require: dependencies(require),
        require_dev: dependencies(require_dev),
        ..Default::default()
    }
}

/// `names` as lock requirements, each at `*`, since no test here turns on a range.
fn dependencies(names: &[&str]) -> phast_core::lock::phast::RequireSection {
    names
        .iter()
        .map(|name| ((*name).to_owned(), "*".to_owned()))
        .collect()
}

/// A workspace member as `toposort` sees it: a manifest, before anything is
/// resolved, so only what the member itself declared.
fn sibling(name: &str, require: &[&str], require_dev: &[&str]) -> Member {
    Member::new(
        PathBuf::from(format!("packages/{name}")),
        PhastManifest {
            package: PackageSection {
                name: Some(name.to_owned()),
                ..Default::default()
            },
            require: RequireSection {
                dependencies: declared(require),
            },
            require_dev: RequireSection {
                dependencies: declared(require_dev),
            },
            ..Default::default()
        },
    )
}

/// `names` as manifest requirements, each at `*`.
fn declared(names: &[&str]) -> IndexMap<String, DependencySpec> {
    names
        .iter()
        .map(|name| ((*name).to_owned(), DependencySpec::Version("*".to_owned())))
        .collect()
}

/// The names a closure returned, which it already sorted.
fn names(packages: &[PackageDef]) -> Vec<&str> {
    packages.iter().map(|p| p.name.as_str()).collect()
}

/// Index of `name` in the order, panicking if absent.
fn rank(order: &[&str], name: &str) -> usize {
    order.iter().position(|n| *n == name).unwrap()
}

// ---------------------------------------------------------------------------
// closure: which packages a member needs
// ---------------------------------------------------------------------------

#[test]
fn a_member_that_is_not_in_the_lock_needs_nothing() {
    let resolved = lock(vec![], vec![]);
    assert!(closure(&resolved, "acme/missing", false).is_empty());
}

#[test]
fn a_member_with_no_requirements_needs_nothing() {
    let resolved = lock(vec![member("acme/core", &[], &[])], vec![]);
    assert!(closure(&resolved, "acme/core", false).is_empty());
}

#[test]
fn a_direct_requirement_is_included() {
    let resolved = lock(
        vec![
            member("acme/core", &["vendor/foo"], &[]),
            external("vendor/foo", &[]),
        ],
        vec![],
    );
    let needed = closure(&resolved, "acme/core", false);
    assert_eq!(names(&needed), ["vendor/foo"]);
}

#[test]
fn a_transitive_requirement_is_included() {
    let resolved = lock(
        vec![
            member("acme/core", &["vendor/foo"], &[]),
            external("vendor/foo", &["vendor/bar"]),
            external("vendor/bar", &[]),
        ],
        vec![],
    );
    assert_eq!(
        names(&closure(&resolved, "acme/core", false)),
        ["vendor/bar", "vendor/foo"],
    );
}

#[test]
fn a_member_recorded_in_the_dev_section_is_still_found() {
    // Which list a member sits in is decided by how the root reached it, not by
    // what the member itself needs, so its closure is computed the same way.
    let resolved = lock(
        vec![external("vendor/foo", &[])],
        vec![member("acme/tool", &["vendor/foo"], &[])],
    );
    let needed = closure(&resolved, "acme/tool", false);
    assert_eq!(names(&needed), ["vendor/foo"]);
}

#[test]
fn a_required_sibling_is_traversed_but_not_returned() {
    let resolved = lock(
        vec![
            member("acme/core", &["acme/logger"], &[]),
            member("acme/logger", &[], &[]),
        ],
        vec![],
    );
    // A member is a source tree, not something to install into vendor/.
    assert!(closure(&resolved, "acme/core", false).is_empty());
}

#[test]
fn a_required_siblings_own_requirements_are_included() {
    let resolved = lock(
        vec![
            member("acme/core", &["acme/logger"], &[]),
            member("acme/logger", &["vendor/psr-log"], &[]),
            external("vendor/psr-log", &[]),
        ],
        vec![],
    );
    // Traversing the sibling is the point: code in core runs against what logger
    // needs, so those packages have to be installed for core.
    assert_eq!(
        names(&closure(&resolved, "acme/core", false)),
        ["vendor/psr-log"],
    );
}

#[test]
fn the_result_is_sorted_by_name() {
    let resolved = lock(
        vec![
            member("acme/core", &["vendor/beta", "vendor/alpha"], &[]),
            external("vendor/alpha", &[]),
            external("vendor/beta", &[]),
        ],
        vec![],
    );
    assert_eq!(
        names(&closure(&resolved, "acme/core", false)),
        ["vendor/alpha", "vendor/beta"],
    );
}

#[test]
fn a_package_reached_twice_appears_once() {
    // vendor/shared is a dep of both acme/core directly and transitively via vendor/foo.
    let resolved = lock(
        vec![
            member("acme/core", &["vendor/foo", "vendor/shared"], &[]),
            external("vendor/foo", &["vendor/shared"]),
            external("vendor/shared", &[]),
        ],
        vec![],
    );
    assert_eq!(
        names(&closure(&resolved, "acme/core", false)),
        ["vendor/foo", "vendor/shared"],
    );
}

#[test]
fn a_cycle_between_two_packages_terminates() {
    let resolved = lock(
        vec![
            member("acme/core", &["vendor/a"], &[]),
            external("vendor/a", &["vendor/b"]),
            external("vendor/b", &["vendor/a"]),
        ],
        vec![],
    );
    // A resolved lock should not contain one, but the walk cannot assume that and
    // hang if it does.
    assert_eq!(
        names(&closure(&resolved, "acme/core", false)),
        ["vendor/a", "vendor/b"],
    );
}

#[test]
fn a_member_requiring_itself_is_passed_over() {
    let resolved = lock(
        vec![
            member("acme/core", &["acme/core", "vendor/foo"], &[]),
            external("vendor/foo", &[]),
        ],
        vec![],
    );
    // The seed is marked visited before the walk starts, so it cannot come back
    // around as one of its own dependencies.
    let needed = closure(&resolved, "acme/core", false);
    assert_eq!(names(&needed), ["vendor/foo"]);
}

#[test]
fn a_platform_requirement_is_silently_skipped() {
    // `php` appears as a require key but is never a lock entry, and neither is an
    // optional dependency that went unresolved.
    let resolved = lock(
        vec![
            member("acme/core", &["php", "vendor/foo"], &[]),
            external("vendor/foo", &[]),
        ],
        vec![],
    );
    let needed = closure(&resolved, "acme/core", false);
    assert_eq!(names(&needed), ["vendor/foo"]);
}

// ---------------------------------------------------------------------------
// closure: following dev requirements
// ---------------------------------------------------------------------------

#[test]
fn a_dev_requirement_is_excluded_without_include_dev() {
    let resolved = lock(
        vec![member("acme/core", &[], &["vendor/phpunit"])],
        vec![external("vendor/phpunit", &[])],
    );
    assert!(closure(&resolved, "acme/core", false).is_empty());
}

#[test]
fn a_dev_requirement_is_included_with_include_dev() {
    let resolved = lock(
        vec![member("acme/core", &[], &["vendor/phpunit"])],
        vec![external("vendor/phpunit", &[])],
    );
    assert_eq!(
        names(&closure(&resolved, "acme/core", true)),
        ["vendor/phpunit"],
    );
}

#[test]
fn a_dev_requirements_own_requirements_are_followed() {
    let resolved = lock(
        vec![member("acme/core", &[], &["vendor/phpunit"])],
        vec![
            external("vendor/phpunit", &["vendor/deep"]),
            external("vendor/deep", &[]),
        ],
    );
    // Only the seed's `require-dev` seeds the walk; from there it follows `require`
    // edges, so what the test tool itself needs comes along.
    assert_eq!(
        names(&closure(&resolved, "acme/core", true)),
        ["vendor/deep", "vendor/phpunit"],
    );
}

#[test]
fn a_siblings_dev_requirements_are_not_followed() {
    // When computing acme/core's closure, we should NOT pull in acme/logger's dev dep
    // even with include_dev=true -- that's logger's test infrastructure, not core's.
    let resolved = lock(
        vec![
            member("acme/core", &["acme/logger"], &[]),
            member("acme/logger", &[], &["vendor/phpunit"]),
        ],
        vec![external("vendor/phpunit", &[])],
    );
    assert!(
        closure(&resolved, "acme/core", true).is_empty(),
        "sibling dev deps must not bleed into peer closure",
    );
}

// ---------------------------------------------------------------------------
// toposort: ordering members by what they require
// ---------------------------------------------------------------------------

#[test]
fn a_required_sibling_comes_first() {
    // a requires b, b requires c  =>  c before b before a
    let members = vec![
        sibling("acme/a", &["acme/b"], &[]),
        sibling("acme/b", &["acme/c"], &[]),
        sibling("acme/c", &[], &[]),
    ];
    let order = toposort(&members).unwrap();
    assert!(rank(&order, "acme/c") < rank(&order, "acme/b"));
    assert!(rank(&order, "acme/b") < rank(&order, "acme/a"));
}

#[test]
fn a_diamond_orders_the_shared_sibling_first() {
    let members = vec![
        sibling("acme/a", &["acme/b", "acme/c"], &[]),
        sibling("acme/b", &["acme/d"], &[]),
        sibling("acme/c", &["acme/d"], &[]),
        sibling("acme/d", &[], &[]),
    ];
    let order = toposort(&members).unwrap();
    assert!(rank(&order, "acme/d") < rank(&order, "acme/b"));
    assert!(rank(&order, "acme/d") < rank(&order, "acme/c"));
    assert!(rank(&order, "acme/b") < rank(&order, "acme/a"));
    assert!(rank(&order, "acme/c") < rank(&order, "acme/a"));
}

#[test]
fn the_order_is_the_same_on_every_run() {
    // The graph is built in a HashMap, so the roots are sorted before the walk.
    // Without that, `phast install` would visit members in a different order each
    // time and a reported cycle would name a different starting member.
    let members = vec![
        sibling("acme/a", &["acme/b", "acme/c"], &[]),
        sibling("acme/b", &["acme/d"], &[]),
        sibling("acme/c", &["acme/d"], &[]),
        sibling("acme/d", &[], &[]),
        sibling("acme/e", &[], &[]),
    ];

    let first = toposort(&members).unwrap();
    for _ in 0..8 {
        assert_eq!(toposort(&members).unwrap(), first);
    }
}

#[test]
fn a_workspace_with_no_members_orders_to_nothing() {
    assert!(toposort(&[]).unwrap().is_empty());
}

#[test]
fn a_require_cycle_is_rejected_naming_the_whole_path() {
    let members = vec![
        sibling("acme/a", &["acme/b"], &[]),
        sibling("acme/b", &["acme/a"], &[]),
    ];

    // The path is what makes the error actionable: it says which edge to move.
    let error = toposort(&members).unwrap_err().to_string();
    assert!(
        error.contains("require cycle: acme/a -> acme/b -> acme/a"),
        "{error}",
    );
    assert!(error.contains("[require-dev]"), "{error}");
}

#[test]
fn a_longer_cycle_is_reported_in_full() {
    let members = vec![
        sibling("acme/a", &["acme/b"], &[]),
        sibling("acme/b", &["acme/c"], &[]),
        sibling("acme/c", &["acme/a"], &[]),
    ];

    let error = toposort(&members).unwrap_err().to_string();
    assert!(
        error.contains("acme/a -> acme/b -> acme/c -> acme/a"),
        "{error}",
    );
}

#[test]
fn a_member_requiring_itself_is_a_cycle() {
    let members = vec![sibling("acme/a", &["acme/a"], &[])];
    let error = toposort(&members).unwrap_err().to_string();
    assert!(error.contains("acme/a -> acme/a"), "{error}");
}

#[test]
fn a_cycle_closed_only_through_require_dev_is_allowed() {
    // a requires b (prod); b depends on a only through require-dev.
    // A member is usable without the siblings its tests need.
    let members = vec![
        sibling("acme/a", &["acme/b"], &[]),
        sibling("acme/b", &[], &["acme/a"]),
    ];
    assert!(toposort(&members).is_ok());
}

#[test]
fn a_requirement_on_a_non_member_is_not_an_edge() {
    // Registry packages and platform requirements are not siblings to order.
    let members = vec![sibling("acme/a", &["vendor/foo", "php"], &[])];
    assert_eq!(toposort(&members).unwrap(), vec!["acme/a"]);
}

#[test]
fn a_member_declaring_no_name_is_omitted() {
    let members = vec![
        sibling("acme/a", &[], &[]),
        Member::new(PathBuf::from("packages/anon"), PhastManifest::default()),
    ];
    assert_eq!(toposort(&members).unwrap(), vec!["acme/a"]);
}
