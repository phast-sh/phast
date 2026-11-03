use std::collections::BTreeSet;
use std::path::Path;

use phast_autoload::builder::AutoloadMaps;
use phast_autoload::writer::Writer;
use phast_core::platform::PlatformRequirements;

use tempfile::TempDir;

/// The lock-content suffix the generated bootstrap class names are built from.
const SUFFIX: &str = "abcd1234";

/// Autoload maps with every section empty, for a test to fill in what it needs.
fn maps() -> AutoloadMaps {
    AutoloadMaps::default()
}

// ---------------------------------------------------------------------------
// Which files a write produces
// ---------------------------------------------------------------------------

#[test]
fn a_write_produces_exactly_these_files() {
    let tmp = TempDir::new().unwrap();
    Writer::new(tmp.path(), SUFFIX).write(&maps()).unwrap();

    // Both levels in full, not a sample: a file added or dropped without a
    // matching change here is the drift this test exists to catch.
    assert_eq!(names(tmp.path()), vec!["autoload.php", "phast"]);
    assert_eq!(
        names(&tmp.path().join("phast")),
        vec![
            "ClassLoader.php",
            "InstalledVersions.php",
            "LICENSE",
            "autoload_classmap.php",
            "autoload_files.php",
            "autoload_namespaces.php",
            "autoload_psr4.php",
            "autoload_real.php",
            "autoload_static.php",
            "platform_check.php",
        ],
    );
}

#[test]
fn a_second_write_replaces_the_first() {
    let tmp = TempDir::new().unwrap();
    Writer::new(tmp.path(), SUFFIX).write(&maps()).unwrap();
    let first = read(&tmp, "phast/autoload_real.php");

    // Re-running an install writes over a populated vendor/, so the generated
    // files have to be replaced rather than appended to.
    Writer::new(tmp.path(), SUFFIX).write(&maps()).unwrap();

    assert_eq!(read(&tmp, "phast/autoload_real.php"), first);
}

// ---------------------------------------------------------------------------
// LICENSE: the license the generated loader ships under
// ---------------------------------------------------------------------------

#[test]
fn ships_project_license() {
    let tmp = TempDir::new().unwrap();
    Writer::new(tmp.path(), SUFFIX).write(&maps()).unwrap();

    let content = read(&tmp, "phast/LICENSE");
    assert!(
        content.contains("License"),
        "vendor/phast/LICENSE must carry the project's license",
    );
}

// ---------------------------------------------------------------------------
// autoload.php: the entry point a project requires
// ---------------------------------------------------------------------------

#[test]
fn autoload_entry_requires_real_php() {
    let tmp = TempDir::new().unwrap();
    Writer::new(tmp.path(), SUFFIX).write(&maps()).unwrap();

    let content = read(&tmp, "autoload.php");
    assert!(
        content.contains("autoload_real.php"),
        "entry must require autoload_real.php",
    );
    assert!(
        content.contains("getLoader()"),
        "entry must call getLoader()",
    );
}

// ---------------------------------------------------------------------------
// autoload_real.php: the bootstrap and the Composer aliases
// ---------------------------------------------------------------------------

#[test]
fn autoload_real_references_class_loader() {
    let tmp = TempDir::new().unwrap();
    Writer::new(tmp.path(), SUFFIX).write(&maps()).unwrap();

    let content = read(&tmp, "phast/autoload_real.php");
    assert!(content.contains("ClassLoader.php"));
    assert!(content.contains("Phast\\Autoload\\ClassLoader"));
    assert!(
        content.contains("Composer\\Autoload\\ClassLoader"),
        "alias must be present",
    );
}

#[test]
fn installed_versions_is_aliased_as_composers() {
    let tmp = TempDir::new().unwrap();
    Writer::new(tmp.path(), SUFFIX).write(&maps()).unwrap();

    let alias =
        "class_alias(\\Phast\\InstalledVersions::class, \\Composer\\InstalledVersions::class)";

    let content = read(&tmp, "phast/autoload_real.php");
    assert!(
        content.contains("InstalledVersions.php"),
        "the shim must be required",
    );
    assert!(
        content.contains(alias),
        "packages calling \\Composer\\InstalledVersions must reach the shim: {content}",
    );
}

#[test]
fn a_foreign_composer_installed_versions_is_not_replaced() {
    let tmp = TempDir::new().unwrap();
    Writer::new(tmp.path(), SUFFIX).write(&maps()).unwrap();

    // PHPStan and friends ship their own \Composer\InstalledVersions inside a phar.
    // Aliasing over one that is already loaded is a fatal error, so the alias is
    // guarded and an existing class is reloaded rather than redefined.
    let content = read(&tmp, "phast/autoload_real.php");
    assert!(
        content.contains("class_exists(\\Composer\\InstalledVersions::class, false)"),
        "the alias must be guarded: {content}",
    );
}

#[test]
fn authoritative_defaults_to_false() {
    let tmp = TempDir::new().unwrap();
    Writer::new(tmp.path(), SUFFIX).write(&maps()).unwrap();

    let content = read(&tmp, "phast/autoload_real.php");
    assert!(
        content.contains("setClassMapAuthoritative(false)"),
        "an optimize-only install must not enable authoritative mode",
    );
}

#[test]
fn authoritative_can_be_enabled() {
    let tmp = TempDir::new().unwrap();
    Writer::new(tmp.path(), SUFFIX)
        .authoritative(true)
        .write(&maps())
        .unwrap();

    let content = read(&tmp, "phast/autoload_real.php");
    assert!(
        content.contains("setClassMapAuthoritative(true)"),
        "authoritative(true) must enable authoritative mode in the bootstrap",
    );
}

// ---------------------------------------------------------------------------
// platform_check.php: the guard against a host that cannot run this
// ---------------------------------------------------------------------------

#[test]
fn platform_check_is_inert_without_requirements() {
    let tmp = TempDir::new().unwrap();
    Writer::new(tmp.path(), SUFFIX).write(&maps()).unwrap();

    let content = read(&tmp, "phast/platform_check.php");
    assert!(
        !content.contains("PHP_VERSION_ID") && !content.contains("extension_loaded"),
        "with no requirements the guard must perform no checks: {content}",
    );
}

#[test]
fn platform_check_renders_php_and_extension_checks() {
    let tmp = TempDir::new().unwrap();
    let requirements = PlatformRequirements {
        php: Some(80100),
        extensions: BTreeSet::from(["mbstring".to_owned()]),
    };
    Writer::new(tmp.path(), SUFFIX)
        .platform_check(Some(requirements))
        .write(&maps())
        .unwrap();

    let content = read(&tmp, "phast/platform_check.php");
    assert!(
        content.contains("PHP_VERSION_ID >= 80100"),
        "the php floor must be checked: {content}",
    );
    assert!(
        content.contains("extension_loaded('mbstring')"),
        "each required extension must be checked: {content}",
    );
}

// ---------------------------------------------------------------------------
// autoload_static.php: the maps inlined for one-pass loading
// ---------------------------------------------------------------------------

#[test]
fn autoload_static_contains_psr4_entries() {
    let tmp = TempDir::new().unwrap();
    let mut maps = maps();
    maps.psr4
        .insert("Acme\\".to_owned(), vec!["/vendor/acme/src".to_owned()]);

    Writer::new(tmp.path(), SUFFIX).write(&maps).unwrap();

    let content = read(&tmp, "phast/autoload_static.php");
    assert!(
        content.contains("Acme\\\\"),
        "PSR-4 prefix must appear in static map",
    );
    assert!(content.contains("/vendor/acme/src"));
}

#[test]
fn autoload_static_contains_classmap_entries() {
    let tmp = TempDir::new().unwrap();
    let mut maps = maps();
    maps.classmap.insert(
        "Acme\\Foo\\Bar".to_owned(),
        "/vendor/acme/src/Foo/Bar.php".to_owned(),
    );

    Writer::new(tmp.path(), SUFFIX).write(&maps).unwrap();

    let content = read(&tmp, "phast/autoload_static.php");
    assert!(content.contains("Acme\\\\Foo\\\\Bar"));
    assert!(content.contains("/vendor/acme/src/Foo/Bar.php"));
}

// ---------------------------------------------------------------------------
// Bootstrap class names, which must not depend on the local path
// ---------------------------------------------------------------------------

#[test]
fn class_names_follow_suffix_not_vendor_path() {
    // Two different vendor directories, same suffix: the generated bootstrap
    // class names must be identical, proving they derive from the suffix (the
    // lock content) and not from the local vendor path.
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    Writer::new(a.path(), SUFFIX).write(&maps()).unwrap();
    Writer::new(b.path(), SUFFIX).write(&maps()).unwrap();

    let real_a = std::fs::read_to_string(a.path().join("phast/autoload_real.php")).unwrap();
    let real_b = std::fs::read_to_string(b.path().join("phast/autoload_real.php")).unwrap();

    assert_eq!(
        real_a, real_b,
        "same suffix on different paths must yield identical class names",
    );
    assert!(
        real_a.contains(&format!("PhastAutoloaderInit{SUFFIX}")),
        "the suffix must appear verbatim in the bootstrap class name",
    );
}

// ---------------------------------------------------------------------------
// autoload_psr4.php: the standalone PSR-4 map
// ---------------------------------------------------------------------------

#[test]
fn psr4_php_file_is_valid_return_array() {
    let tmp = TempDir::new().unwrap();
    Writer::new(tmp.path(), SUFFIX).write(&maps()).unwrap();

    let content = read(&tmp, "phast/autoload_psr4.php");
    assert!(content.contains("<?php"));
    assert!(content.contains("return array()") || content.contains("return array(\n"));
}

#[test]
fn psr4_php_file_contains_registered_prefix() {
    let tmp = TempDir::new().unwrap();
    let mut maps = maps();
    maps.psr4
        .insert("MyApp\\".to_owned(), vec!["/vendor/myapp/src".to_owned()]);

    Writer::new(tmp.path(), SUFFIX).write(&maps).unwrap();

    let content = read(&tmp, "phast/autoload_psr4.php");
    assert!(content.contains("MyApp\\\\"));
}

/// The generated file at `rel` under the vendor dir, as text.
fn read(tmp: &TempDir, rel: &str) -> String {
    std::fs::read_to_string(tmp.path().join(rel)).unwrap()
}

/// The entries directly inside `dir`, by name, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
