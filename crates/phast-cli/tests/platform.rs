use std::collections::BTreeMap;

use phast_cli::platform::build_platform_map;
use phast_core::manifest::{Config, Manifest};
use phast_core::package::PackageName;

/// A manifest whose `config.platform` is `platform`, and which requires nothing.
fn manifest(platform: BTreeMap<String, String>) -> Manifest {
    Manifest {
        config: Config {
            platform,
            ..Config::default()
        },
        ..Manifest::default()
    }
}

// ---------------------------------------------------------------------------
// What config.platform puts in the map
// ---------------------------------------------------------------------------

#[test]
fn platform_map_includes_configured_php_version() {
    let platform = BTreeMap::from([("php".to_owned(), "8.2.0".to_owned())]);
    let manifest = manifest(platform);

    let map = build_platform_map(&manifest).unwrap();
    let php = map
        .get(&PackageName::new("php"))
        .expect("php must be in map");
    assert_eq!(php.to_string(), "8.2");
}

#[test]
fn platform_map_includes_configured_extensions() {
    let platform = BTreeMap::from([
        ("php".to_owned(), "8.2.0".to_owned()),
        ("ext-json".to_owned(), "1.0.0".to_owned()),
    ]);
    let manifest = manifest(platform);

    let map = build_platform_map(&manifest).unwrap();
    assert!(
        map.contains_key(&PackageName::new("ext-json")),
        "ext-json from config.platform must be in map",
    );
}

#[test]
fn an_entry_that_does_not_parse_is_dropped() {
    let platform = BTreeMap::from([
        ("php".to_owned(), "8.2.0".to_owned()),
        ("ext-broken".to_owned(), "not-a-version".to_owned()),
    ]);
    let manifest = manifest(platform);

    let map = build_platform_map(&manifest).unwrap();

    // Dropped rather than refused, so one bad entry does not stop the build, and
    // the entries beside it are still honoured.
    assert!(!map.contains_key(&PackageName::new("ext-broken")));
    assert_eq!(
        map.get(&PackageName::new("php")).unwrap().to_string(),
        "8.2",
    );
}

// ---------------------------------------------------------------------------
// What detection fills in, which needs PHP on the machine
// ---------------------------------------------------------------------------

#[test]
fn platform_map_contains_php_when_not_configured() {
    // When config.platform has no `php` entry, build_platform_map detects it
    // from the environment. This test requires PHP to be installed.
    let manifest = manifest(BTreeMap::new());
    let map = build_platform_map(&manifest).unwrap();
    assert!(
        map.contains_key(&PackageName::new("php")),
        "php must always be present in the platform map",
    );
}

#[test]
fn loaded_extensions_are_detected() {
    let manifest = manifest(BTreeMap::new());
    let map = build_platform_map(&manifest).unwrap();

    // `Core` is loaded by every PHP build, so its absence means the detection
    // script never ran rather than that this machine is short an extension.
    assert!(
        map.contains_key(&PackageName::new("ext-core")),
        "detection found no extensions at all: {:?}",
        map.keys().collect::<Vec<_>>(),
    );
}

#[test]
fn configured_php_version_takes_precedence_over_detected() {
    // If php is set in config.platform, detect_php_version() must not be called.
    // We verify this indirectly: the value in the map matches what we configured,
    // not whatever `php --version` would return.
    let platform = BTreeMap::from([("php".to_owned(), "7.4.0".to_owned())]);
    let manifest = manifest(platform);

    let map = build_platform_map(&manifest).unwrap();
    let php = map.get(&PackageName::new("php")).unwrap();
    assert_eq!(php.to_string(), "7.4");
}

#[test]
fn a_php_entry_that_does_not_parse_falls_back_to_detection() {
    let platform = BTreeMap::from([("php".to_owned(), "garbage".to_owned())]);
    let manifest = manifest(platform);

    let map = build_platform_map(&manifest).unwrap();
    let php = map
        .get(&PackageName::new("php"))
        .expect("php must be in map");

    // A typo in `php` is not reported anywhere: the entry is dropped and the
    // interpreter's own version quietly takes its place, so a project pinned to
    // a PHP it does not have resolves against the one it does.
    assert_ne!(php.to_string(), "garbage");
}
