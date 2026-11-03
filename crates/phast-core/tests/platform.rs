use std::collections::BTreeMap;
use std::str::FromStr;

use phast_core::lock::phast::{MetadataSection, PackageDef, PhastLock, RequireSection};
use phast_core::manifest::{Manifest, PlatformCheck};
use phast_core::package::PackageName;
use phast_core::platform::PlatformRequirements;
use phast_version::RequireSpec;

/// A root manifest checking the platform in `mode` and declaring `require` and
/// `require_dev`, each entry a `(name, constraint)` pair.
fn manifest(
    mode: PlatformCheck,
    require: &[(&str, &str)],
    require_dev: &[(&str, &str)],
) -> Manifest {
    let specs = |entries: &[(&str, &str)]| {
        entries
            .iter()
            .map(|&(name, spec)| (PackageName::new(name), RequireSpec::from_str(spec).unwrap()))
            .collect()
    };

    let mut manifest = Manifest {
        require: specs(require),
        require_dev: specs(require_dev),
        ..Default::default()
    };
    manifest.config.platform_check = mode;
    manifest
}

/// A locked package `name` at 1.0.0 requiring `entries`.
fn package(name: &str, entries: &[(&str, &str)]) -> PackageDef {
    let require = entries
        .iter()
        .map(|&(k, v)| (k.to_owned(), v.to_owned()))
        .collect::<BTreeMap<_, _>>();

    PackageDef {
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        require: RequireSection { entries: require },
        ..Default::default()
    }
}

/// A lock holding `packages` and `dev` and nothing else of interest.
fn lock(packages: Vec<PackageDef>, dev: Vec<PackageDef>) -> PhastLock {
    PhastLock {
        version: 1,
        content_hash: "test".to_owned(),
        metadata: MetadataSection {
            generated_by: "test".to_owned(),
            generated_at: "2026-01-01T00:00:00Z".to_owned(),
        },
        packages,
        packages_dev: dev,
        autoload: Default::default(),
        autoload_dev: Default::default(),
    }
}

// ---------------------------------------------------------------------------
// How much of the platform each mode verifies
// ---------------------------------------------------------------------------

#[test]
fn off_mode_yields_no_requirements() {
    let manifest = manifest(PlatformCheck::Off, &[("php", ">=8.2")], &[]);
    let lock = lock(vec![], vec![]);
    assert!(PlatformRequirements::collect(&lock, &manifest, true).is_none());
}

#[test]
fn php_only_ignores_extensions() {
    let manifest = manifest(
        PlatformCheck::Php,
        &[("php", ">=8.2"), ("ext-json", "*")],
        &[],
    );
    let lock = lock(vec![package("acme/dep", &[("ext-mbstring", "*")])], vec![]);

    let reqs = PlatformRequirements::collect(&lock, &manifest, true).unwrap();
    assert_eq!(reqs.php, Some(80200));
    assert!(reqs.extensions.is_empty());
}

#[test]
fn all_mode_unions_extensions_stripped_of_prefix() {
    let manifest = manifest(PlatformCheck::All, &[("ext-json", "*")], &[]);
    let lock = lock(
        vec![package(
            "acme/dep",
            &[("ext-mbstring", "*"), ("ext-json", "*")],
        )],
        vec![],
    );

    let reqs = PlatformRequirements::collect(&lock, &manifest, true).unwrap();
    let extensions: Vec<&str> = reqs.extensions.iter().map(String::as_str).collect();
    assert_eq!(extensions, vec!["json", "mbstring"]);
}

// ---------------------------------------------------------------------------
// What raises the PHP floor
// ---------------------------------------------------------------------------

#[test]
fn php_floor_is_the_highest_across_the_tree() {
    // Root wants >=8.1, a dependency wants ^8.3: the floor is 8.3.
    let manifest = manifest(PlatformCheck::Php, &[("php", ">=8.1")], &[]);
    let lock = lock(vec![package("acme/dep", &[("php", "^8.3")])], vec![]);

    let reqs = PlatformRequirements::collect(&lock, &manifest, true).unwrap();
    assert_eq!(reqs.php, Some(80300));
}

#[test]
fn a_branch_php_constraint_has_no_floor() {
    let manifest = manifest(PlatformCheck::Php, &[("php", "dev-main")], &[]);
    let lock = lock(vec![], vec![]);

    // A branch is a name, not a point on the version line, so there is nothing
    // to compare a running interpreter against.
    let reqs = PlatformRequirements::collect(&lock, &manifest, true).unwrap();
    assert_eq!(reqs.php, None);
}

#[test]
fn a_php_constraint_with_no_lower_bound_has_no_floor() {
    let manifest = manifest(PlatformCheck::Php, &[("php", "<9.0")], &[]);
    let lock = lock(vec![], vec![]);

    // The guard only ever checks a minimum, so a constraint that names a ceiling
    // and no floor contributes nothing to it.
    let reqs = PlatformRequirements::collect(&lock, &manifest, true).unwrap();
    assert_eq!(reqs.php, None);
}

// ---------------------------------------------------------------------------
// Which requirements are counted at all
// ---------------------------------------------------------------------------

#[test]
fn dev_packages_only_count_when_included() {
    let manifest = manifest(PlatformCheck::All, &[], &[]);
    let lock = lock(vec![], vec![package("acme/test", &[("ext-xdebug", "*")])]);

    let without = PlatformRequirements::collect(&lock, &manifest, false).unwrap();
    assert!(without.extensions.is_empty());

    let with = PlatformRequirements::collect(&lock, &manifest, true).unwrap();
    assert!(with.extensions.contains("xdebug"));
}

#[test]
fn the_roots_require_dev_is_never_guarded() {
    let manifest = manifest(
        PlatformCheck::All,
        &[],
        &[("ext-xdebug", "*"), ("php", ">=8.4")],
    );
    let lock = lock(vec![], vec![]);

    // A known limitation rather than a choice: only the root's `require` is read,
    // so a platform need declared just for development is never verified, even
    // with dev packages included. Note it drops the php floor too, not only the
    // extension.
    let reqs = PlatformRequirements::collect(&lock, &manifest, true).unwrap();
    assert_eq!(reqs.php, None);
    assert!(reqs.extensions.is_empty());
}

#[test]
fn an_unparseable_constraint_is_skipped() {
    let manifest = manifest(PlatformCheck::All, &[], &[]);
    let lock = lock(
        vec![package("acme/dep", &[("php", "!!!"), ("ext-json", "*")])],
        vec![],
    );

    // Dropped rather than fatal, and the entries beside it still count: a lock
    // Phast did not write should not stop the guard being generated.
    let reqs = PlatformRequirements::collect(&lock, &manifest, true).unwrap();
    assert_eq!(reqs.php, None);
    assert!(reqs.extensions.contains("json"));
}
