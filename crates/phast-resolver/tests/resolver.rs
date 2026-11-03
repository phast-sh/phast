use std::collections::{HashMap, HashSet};

use phast_core::package::PackageName;
use phast_resolver::provider::memory::MemoryProvider;
use phast_resolver::{Requirement, Resolution, ResolveError, ResolveOptions, resolve};
use phast_version::{
    RequireSpec, VersionSpec,
    parser::parse_constraint,
    stability::Stability,
    version::{NumericVersion, Version},
};

/// A stable release at `major.minor.patch`.
fn stable(major: u64, minor: u64, patch: u64) -> Version {
    Version::Numeric(NumericVersion {
        segments: [major, minor, patch, 0],
        stability: Stability::Stable,
        stability_seq: 0,
        precision: 3,
    })
}

/// The package `name`, which the assertions look solutions up by.
fn package(name: &str) -> PackageName {
    PackageName::new(name)
}

/// A `[require]` entry on `name` at `constraint`.
fn requirement(name: &str, constraint: &str) -> Requirement {
    Requirement {
        name: package(name),
        spec: RequireSpec::Constraint {
            constraint: parse_constraint(constraint).unwrap(),
            stability: None,
        },
        is_dev: false,
    }
}

/// The same as a `[require-dev]` entry, which is what makes the solve two-pass.
fn dev_requirement(name: &str, constraint: &str) -> Requirement {
    Requirement {
        is_dev: true,
        ..requirement(name, constraint)
    }
}

/// One entry in a package's own dependency list.
fn dependency(name: &str, constraint: &str) -> (String, phast_version::Constraint) {
    (name.to_owned(), parse_constraint(constraint).unwrap())
}

/// The versions `requirements` resolve to, with no platform and default options.
async fn run(
    provider: MemoryProvider,
    requirements: Vec<Requirement>,
) -> Result<HashMap<PackageName, Version>, ResolveError> {
    let result = resolution(provider, requirements, ResolveOptions::default()).await?;
    Ok(result.packages)
}

/// The whole resolution, for a test that varies `options` or reads the dev split.
async fn resolution(
    provider: MemoryProvider,
    requirements: Vec<Requirement>,
    options: ResolveOptions,
) -> Result<Resolution, ResolveError> {
    resolve(requirements, provider, HashMap::new(), options).await
}

/// A `[conflict]` entry forbidding `constraint` of `name`.
fn forbid(name: &str, constraint: &str) -> HashMap<PackageName, RequireSpec> {
    HashMap::from([(
        package(name),
        RequireSpec::Constraint {
            constraint: parse_constraint(constraint).unwrap(),
            stability: None,
        },
    )])
}

/// The names in `packages`, sorted, since a resolution is a `HashMap`.
fn names<'a>(packages: impl Iterator<Item = &'a PackageName>) -> Vec<String> {
    let mut names: Vec<String> = packages.map(|name| name.as_str().to_owned()).collect();
    names.sort();
    names
}

// ---------------------------------------------------------------------------
// Picking a version
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_single_requirement_resolves_to_its_one_release() {
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(1, 2, 3), vec![]);

    let solution = run(provider, vec![requirement("vendor/a", "^1.0")])
        .await
        .unwrap();
    assert_eq!(solution[&package("vendor/a")], stable(1, 2, 3));
}

#[tokio::test]
async fn the_newest_satisfying_version_is_picked() {
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(1, 0, 0), vec![]);
    provider.add_package("vendor/a", stable(1, 1, 0), vec![]);
    provider.add_package("vendor/a", stable(1, 2, 0), vec![]);
    provider.add_package("vendor/a", stable(2, 0, 0), vec![]);

    let solution = run(provider, vec![requirement("vendor/a", "^1.0")])
        .await
        .unwrap();
    assert_eq!(solution[&package("vendor/a")], stable(1, 2, 0));
}

#[tokio::test]
async fn a_transitive_dependency_is_resolved_too() {
    let mut provider = MemoryProvider::new();
    provider.add_package(
        "vendor/a",
        stable(1, 0, 0),
        vec![dependency("vendor/b", "^2.0")],
    );
    provider.add_package("vendor/b", stable(2, 0, 0), vec![]);
    provider.add_package("vendor/b", stable(2, 1, 0), vec![]);

    let solution = run(provider, vec![requirement("vendor/a", "^1.0")])
        .await
        .unwrap();
    assert_eq!(solution[&package("vendor/a")], stable(1, 0, 0));
    assert_eq!(solution[&package("vendor/b")], stable(2, 1, 0));
}

#[tokio::test]
async fn a_package_two_dependents_share_satisfies_both_of_them() {
    // root -> a ^1.0, b ^1.0
    // a 1.0 -> c ^1.0
    // b 1.0 -> c ^1.1
    // c has 1.0, 1.1, 1.2 -> should pick 1.2 satisfying both ^1.0 and ^1.1
    let mut provider = MemoryProvider::new();
    provider.add_package(
        "vendor/a",
        stable(1, 0, 0),
        vec![dependency("vendor/c", "^1.0")],
    );
    provider.add_package(
        "vendor/b",
        stable(1, 0, 0),
        vec![dependency("vendor/c", "^1.1")],
    );
    provider.add_package("vendor/c", stable(1, 0, 0), vec![]);
    provider.add_package("vendor/c", stable(1, 1, 0), vec![]);
    provider.add_package("vendor/c", stable(1, 2, 0), vec![]);

    let solution = run(
        provider,
        vec![
            requirement("vendor/a", "^1.0"),
            requirement("vendor/b", "^1.0"),
        ],
    )
    .await
    .unwrap();

    assert_eq!(solution[&package("vendor/c")], stable(1, 2, 0));
}

// ---------------------------------------------------------------------------
// A virtual package another package satisfies
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_provide_declaration_satisfies_a_virtual_requirement() {
    // vendor/consumer requires virtual/iface ^1.0 (not on any registry).
    // vendor/provider declares provide: [("virtual/iface", 1.0.0)].
    // Root requires both directly so vendor/provider is fetched and its
    // provide entry is indexed before the solver asks for virtual/iface versions.
    let mut provider = MemoryProvider::new();
    provider.add_package(
        "vendor/consumer",
        stable(1, 0, 0),
        vec![dependency("virtual/iface", "^1.0")],
    );
    provider.add_package_full(
        "vendor/provider",
        stable(1, 0, 0),
        vec![],
        vec![("virtual/iface".to_owned(), stable(1, 0, 0))],
        vec![],
    );

    let solution = run(
        provider,
        vec![
            requirement("vendor/consumer", "^1.0"),
            requirement("vendor/provider", "^1.0"),
        ],
    )
    .await
    .unwrap();

    assert_eq!(solution[&package("vendor/consumer")], stable(1, 0, 0));
    assert_eq!(solution[&package("vendor/provider")], stable(1, 0, 0));
    // virtual/iface is not a real package and must not appear in the output.
    assert!(!solution.contains_key(&package("virtual/iface")));
}

#[tokio::test]
async fn a_replace_declaration_satisfies_a_requirement_for_the_replaced_package() {
    // vendor/consumer requires vendor/old-api ^1.0 (not on any registry).
    // vendor/new-impl declares replace: [("vendor/old-api", 1.0.0)].
    // Root requires both so vendor/new-impl is fetched and its replace entry
    // is indexed before the solver asks for vendor/old-api versions.
    let mut provider = MemoryProvider::new();
    provider.add_package(
        "vendor/consumer",
        stable(1, 0, 0),
        vec![dependency("vendor/old-api", "^1.0")],
    );
    provider.add_package_full(
        "vendor/new-impl",
        stable(1, 0, 0),
        vec![],
        vec![],
        vec![("vendor/old-api".to_owned(), stable(1, 0, 0))],
    );

    let solution = run(
        provider,
        vec![
            requirement("vendor/consumer", "^1.0"),
            requirement("vendor/new-impl", "^1.0"),
        ],
    )
    .await
    .unwrap();

    assert_eq!(solution[&package("vendor/consumer")], stable(1, 0, 0));
    assert_eq!(solution[&package("vendor/new-impl")], stable(1, 0, 0));
    // vendor/old-api is satisfied via replace and must not appear as a real package.
    assert!(!solution.contains_key(&package("vendor/old-api")));
}

#[tokio::test]
async fn a_replaced_package_is_dropped_even_when_the_registry_serves_it() {
    // The Laravel case: laravel/framework replaces illuminate/auth, which is ALSO a
    // real, resolvable package on the registry (a consumer requires it). Composer
    // satisfies that requirement via the framework and never installs the standalone
    // package; Phast must do the same and not over-resolve it.
    let mut provider = MemoryProvider::new();
    provider.add_package("illuminate/auth", stable(11, 0, 0), vec![]);
    provider.add_package(
        "vendor/consumer",
        stable(1, 0, 0),
        vec![dependency("illuminate/auth", "^11.0")],
    );
    provider.add_package_full(
        "laravel/framework",
        stable(11, 0, 0),
        vec![],
        vec![],
        vec![("illuminate/auth".to_owned(), stable(11, 0, 0))],
    );

    let solution = run(
        provider,
        vec![
            requirement("vendor/consumer", "^1.0"),
            requirement("laravel/framework", "^11.0"),
        ],
    )
    .await
    .unwrap();

    assert_eq!(solution[&package("laravel/framework")], stable(11, 0, 0));
    assert_eq!(solution[&package("vendor/consumer")], stable(1, 0, 0));
    // Replaced by the resolved framework: dropped despite existing on the registry.
    assert!(!solution.contains_key(&package("illuminate/auth")));
}

// ---------------------------------------------------------------------------
// A virtual package the root itself satisfies
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_root_provide_satisfies_a_required_virtual_interface() {
    // A dependency requires virtual/iface ^1.0, which exists on no registry. The
    // root project declares it provides virtual/iface at 1.0.0, so the requirement
    // is satisfied by the root and the interface is never installed.
    let mut provider = MemoryProvider::new();
    provider.add_package(
        "vendor/consumer",
        stable(1, 0, 0),
        vec![dependency("virtual/iface", "^1.0")],
    );

    let result = resolution(
        provider,
        vec![requirement("vendor/consumer", "^1.0")],
        ResolveOptions {
            provide: HashMap::from([(
                package("virtual/iface"),
                VersionSpec::Exact(stable(1, 0, 0)),
            )]),
            ..ResolveOptions::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(
        result.packages[&package("vendor/consumer")],
        stable(1, 0, 0),
    );
    // Provided by the root, not a real package: must not appear in the output.
    assert!(!result.packages.contains_key(&package("virtual/iface")));
}

#[tokio::test]
async fn a_root_replace_shadows_a_real_registry_package() {
    // A dependency requires illuminate/auth ^11.0, which IS a real registry package.
    // The root declares it replaces illuminate/auth, so the standalone package is
    // never queried or installed; the requirement is satisfied by the root.
    let mut provider = MemoryProvider::new();
    provider.add_package("illuminate/auth", stable(11, 0, 0), vec![]);
    provider.add_package(
        "vendor/consumer",
        stable(1, 0, 0),
        vec![dependency("illuminate/auth", "^11.0")],
    );

    let result = resolution(
        provider,
        vec![requirement("vendor/consumer", "^1.0")],
        ResolveOptions {
            replace: HashMap::from([(
                package("illuminate/auth"),
                VersionSpec::Exact(stable(11, 0, 0)),
            )]),
            ..ResolveOptions::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(
        result.packages[&package("vendor/consumer")],
        stable(1, 0, 0),
    );
    // Replaced by the root: dropped despite existing on the registry.
    assert!(!result.packages.contains_key(&package("illuminate/auth")));
}

#[tokio::test]
async fn a_root_replace_at_self_version_resolves_to_the_root_version() {
    // A monorepo root replaces a sub-package at self.version. With the root's own
    // version (3.2.0) plumbed in, self.version resolves to it, so a dependency
    // requiring the sub-package at ^3.0 is satisfied by the root and dropped.
    let mut provider = MemoryProvider::new();
    provider.add_package(
        "vendor/consumer",
        stable(1, 0, 0),
        vec![dependency("acme/sub", "^3.0")],
    );

    let result = resolution(
        provider,
        vec![requirement("vendor/consumer", "^1.0")],
        ResolveOptions {
            root_version: Some(stable(3, 2, 0)),
            replace: HashMap::from([(package("acme/sub"), VersionSpec::SelfVersion)]),
            ..ResolveOptions::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(
        result.packages[&package("vendor/consumer")],
        stable(1, 0, 0),
    );
    // self.version resolved to the root's 3.2.0, which satisfies ^3.0: dropped.
    assert!(!result.packages.contains_key(&package("acme/sub")));
}

#[tokio::test]
async fn a_root_provide_at_any_resolves_to_the_root_version() {
    // `*` and `self.version` both mean the root's own version, so a bare `*` in
    // [provide] still has to satisfy a numeric requirement on the interface.
    let mut provider = MemoryProvider::new();
    provider.add_package(
        "vendor/consumer",
        stable(1, 0, 0),
        vec![dependency("virtual/iface", "^3.0")],
    );

    let result = resolution(
        provider,
        vec![requirement("vendor/consumer", "^1.0")],
        ResolveOptions {
            root_version: Some(stable(3, 2, 0)),
            provide: HashMap::from([(package("virtual/iface"), VersionSpec::Any)]),
            ..ResolveOptions::default()
        },
    )
    .await
    .unwrap();

    assert!(!result.packages.contains_key(&package("virtual/iface")));
}

// ---------------------------------------------------------------------------
// Versions the manifest forbids
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_forbidden_version_is_passed_over_for_an_older_one() {
    // Three versions available; conflict blocks >=2.0.
    // Without conflict the resolver would pick 2.0.0 (highest).
    // With conflict it must fall back to 1.2.0.
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(1, 0, 0), vec![]);
    provider.add_package("vendor/a", stable(1, 2, 0), vec![]);
    provider.add_package("vendor/a", stable(2, 0, 0), vec![]);

    let result = resolution(
        provider,
        vec![requirement("vendor/a", "*")],
        ResolveOptions {
            conflict: forbid("vendor/a", ">=2.0"),
            ..ResolveOptions::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(result.packages[&package("vendor/a")], stable(1, 2, 0));
}

#[tokio::test]
async fn forbidding_every_published_version_fails_the_solve() {
    // Only version available is 2.0.0, which is blocked by conflict.
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(2, 0, 0), vec![]);

    let result = resolution(
        provider,
        vec![requirement("vendor/a", "*")],
        ResolveOptions {
            conflict: forbid("vendor/a", ">=2.0"),
            ..ResolveOptions::default()
        },
    )
    .await;

    assert!(matches!(result, Err(ResolveError::Conflict(_))));
}

#[tokio::test]
async fn a_conflict_on_one_package_leaves_the_others_alone() {
    // Conflict on vendor/b must not interfere with vendor/a resolution.
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(1, 0, 0), vec![]);

    let result = resolution(
        provider,
        vec![requirement("vendor/a", "^1.0")],
        ResolveOptions {
            conflict: forbid("vendor/b", ">=1.0"),
            ..ResolveOptions::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(result.packages[&package("vendor/a")], stable(1, 0, 0));
    assert!(!result.packages.contains_key(&package("vendor/b")));
}

#[tokio::test]
async fn a_conflict_at_self_version_resolves_to_the_root_version() {
    // A monorepo root forbidding its own version of a package: `self.version` is
    // read against the root's 3.0.0, so only that release is excluded.
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(2, 0, 0), vec![]);
    provider.add_package("vendor/a", stable(3, 0, 0), vec![]);

    let result = resolution(
        provider,
        vec![requirement("vendor/a", "*")],
        ResolveOptions {
            root_version: Some(stable(3, 0, 0)),
            conflict: HashMap::from([(package("vendor/a"), RequireSpec::SelfVersion)]),
            ..ResolveOptions::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(result.packages[&package("vendor/a")], stable(2, 0, 0));
}

// ---------------------------------------------------------------------------
// Reporting a solve that cannot succeed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_dependency_no_release_satisfies_is_a_conflict() {
    // a 1.0 requires b ^1.0; b has only 2.0
    let mut provider = MemoryProvider::new();
    provider.add_package(
        "vendor/a",
        stable(1, 0, 0),
        vec![dependency("vendor/b", "^1.0")],
    );
    provider.add_package("vendor/b", stable(2, 0, 0), vec![]);

    let result = run(provider, vec![requirement("vendor/a", "^1.0")]).await;
    assert!(matches!(result, Err(ResolveError::Conflict(_))));
}

#[tokio::test]
async fn two_root_requirements_that_cannot_both_hold_are_a_conflict() {
    // root requires a ^1.0 AND a ^2.0 simultaneously
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(1, 0, 0), vec![]);
    provider.add_package("vendor/a", stable(2, 0, 0), vec![]);

    let result = run(
        provider,
        vec![
            requirement("vendor/a", "^1.0"),
            requirement("vendor/a", "^2.0"),
        ],
    )
    .await;
    assert!(matches!(result, Err(ResolveError::Conflict(_))));
}

// ---------------------------------------------------------------------------
// Holding versions steady across a re-resolve
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_locked_pin_holds_a_package_at_its_locked_version() {
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(1, 0, 0), vec![]);
    provider.add_package("vendor/a", stable(1, 1, 0), vec![]);
    provider.add_package("vendor/a", stable(1, 2, 0), vec![]);

    let result = resolution(
        provider,
        vec![requirement("vendor/a", "^1.0")],
        ResolveOptions {
            locked_pins: HashMap::from([(package("vendor/a"), stable(1, 1, 0))]),
            ..ResolveOptions::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(result.packages[&package("vendor/a")], stable(1, 1, 0));
}

#[tokio::test]
async fn a_locked_pin_outside_the_declared_range_is_a_conflict() {
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(1, 0, 0), vec![]);
    provider.add_package("vendor/a", stable(2, 0, 0), vec![]);

    // A pin is injected as an exact constraint, so one the manifest has since
    // moved away from fails loudly rather than being quietly ignored. This is
    // the difference between a pin and a preference.
    let result = resolution(
        provider,
        vec![requirement("vendor/a", "^1.0")],
        ResolveOptions {
            locked_pins: HashMap::from([(package("vendor/a"), stable(2, 0, 0))]),
            ..ResolveOptions::default()
        },
    )
    .await;

    assert!(matches!(result, Err(ResolveError::Conflict(_))));
}

#[tokio::test]
async fn a_preference_is_taken_when_it_is_still_in_range() {
    // Three versions available; preference is 1.1.0.
    // Without preferences the resolver would pick 1.2.0 (highest).
    // With preferences it should pick 1.1.0 to keep the lock stable.
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(1, 0, 0), vec![]);
    provider.add_package("vendor/a", stable(1, 1, 0), vec![]);
    provider.add_package("vendor/a", stable(1, 2, 0), vec![]);

    let result = resolution(
        provider,
        vec![requirement("vendor/a", "^1.0")],
        ResolveOptions {
            preferences: HashMap::from([(package("vendor/a"), stable(1, 1, 0))]),
            ..ResolveOptions::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(result.packages[&package("vendor/a")], stable(1, 1, 0));
}

#[tokio::test]
async fn a_preference_outside_the_range_falls_back_to_the_newest() {
    // Preferred version 0.9.0 is outside ^1.0, so the resolver should
    // fall back to the highest satisfying version (1.2.0).
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(1, 0, 0), vec![]);
    provider.add_package("vendor/a", stable(1, 1, 0), vec![]);
    provider.add_package("vendor/a", stable(1, 2, 0), vec![]);

    let result = resolution(
        provider,
        vec![requirement("vendor/a", "^1.0")],
        ResolveOptions {
            preferences: HashMap::from([(package("vendor/a"), stable(0, 9, 0))]),
            ..ResolveOptions::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(result.packages[&package("vendor/a")], stable(1, 2, 0));
}

// ---------------------------------------------------------------------------
// Platform requirements
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_platform_version_that_clears_the_floor_resolves() {
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(1, 0, 0), vec![dependency("php", "^8.1")]);

    let platform = HashMap::from([(package("php"), stable(8, 2, 0))]);
    let result = resolve(
        vec![requirement("vendor/a", "^1.0")],
        provider,
        platform,
        ResolveOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(result.packages[&package("vendor/a")], stable(1, 0, 0));
    // Platform packages are constraints, not installables: never in the output.
    assert!(!result.packages.contains_key(&package("php")));
}

#[tokio::test]
async fn a_platform_version_below_the_floor_is_a_conflict() {
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(1, 0, 0), vec![dependency("php", "^8.1")]);

    let platform = HashMap::from([(package("php"), stable(7, 4, 0))]);
    let result = resolve(
        vec![requirement("vendor/a", "^1.0")],
        provider,
        platform,
        ResolveOptions::default(),
    )
    .await;

    assert!(matches!(result, Err(ResolveError::Conflict(_))));
}

// ---------------------------------------------------------------------------
// Splitting dev packages from production
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_package_only_a_dev_requirement_reaches_is_tagged_dev() {
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/shared", stable(1, 0, 0), vec![]);
    provider.add_package("vendor/tool-only", stable(1, 0, 0), vec![]);
    provider.add_package(
        "vendor/prod",
        stable(1, 0, 0),
        vec![dependency("vendor/shared", "^1.0")],
    );
    provider.add_package(
        "vendor/tool",
        stable(1, 0, 0),
        vec![
            dependency("vendor/shared", "^1.0"),
            dependency("vendor/tool-only", "^1.0"),
        ],
    );

    let result = resolution(
        provider,
        vec![
            requirement("vendor/prod", "^1.0"),
            dev_requirement("vendor/tool", "^1.0"),
        ],
        ResolveOptions::default(),
    )
    .await
    .unwrap();

    // The split is by difference: a second, prod-only solve is subtracted from the
    // full one, so `vendor/shared` counts as production despite the dev tool also
    // needing it, and the tool's own exclusive dependency comes along as dev.
    let installed = names(result.packages.keys());
    let expected = [
        "vendor/prod",
        "vendor/shared",
        "vendor/tool",
        "vendor/tool-only",
    ];
    assert_eq!(installed, expected);
    assert_eq!(
        names(result.dev_package_names.iter()),
        ["vendor/tool", "vendor/tool-only"],
    );
    assert_eq!(
        names(result.production_packages().map(|(name, _)| name)),
        ["vendor/prod", "vendor/shared"],
    );
}

#[tokio::test]
async fn a_solve_with_no_dev_requirement_tags_nothing_dev() {
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(1, 0, 0), vec![]);

    // The second pass only runs when there is a dev requirement to subtract, so
    // a production install reports an empty set rather than an unknown one.
    let result = resolution(
        provider,
        vec![requirement("vendor/a", "^1.0")],
        ResolveOptions::default(),
    )
    .await
    .unwrap();

    assert!(result.dev_package_names.is_empty());
}

#[tokio::test]
async fn a_pin_for_a_known_dev_package_is_dropped_from_a_production_solve() {
    // The previous lock pinned vendor/tool and recorded it as dev-only. Installing
    // without dev must not pull it back in: a pin is injected as a root-level
    // requirement, so leaving it in would install a dev package in a prod solve.
    let pins = || {
        HashMap::from([
            (package("vendor/a"), stable(1, 0, 0)),
            (package("vendor/tool"), stable(1, 0, 0)),
        ])
    };
    let registry = || {
        let mut provider = MemoryProvider::new();
        provider.add_package("vendor/a", stable(1, 0, 0), vec![]);
        provider.add_package("vendor/tool", stable(1, 0, 0), vec![]);
        provider
    };

    let result = resolution(
        registry(),
        vec![requirement("vendor/a", "^1.0")],
        ResolveOptions {
            locked_pins: pins(),
            dev_package_names: HashSet::from([package("vendor/tool")]),
            ..ResolveOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(names(result.packages.keys()), ["vendor/a"]);

    // Without the hint the same pin is honored, which is what makes the filter
    // load-bearing rather than incidental.
    let unfiltered = resolution(
        registry(),
        vec![requirement("vendor/a", "^1.0")],
        ResolveOptions {
            locked_pins: pins(),
            ..ResolveOptions::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        names(unfiltered.packages.keys()),
        ["vendor/a", "vendor/tool"],
    );
}
