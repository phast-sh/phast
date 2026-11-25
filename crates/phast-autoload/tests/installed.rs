use phast_autoload::installed::{self, Installed};
use phast_core::lock::phast::{
    DistSection, DistType, ExtraSection, MetadataSection, PackageDef, PhastLock, SourceRef,
};
use phast_core::manifest::VcsType;
use phast_core::package::PackageName;

use indoc::indoc;
use tempfile::TempDir;

/// The record Composer 2 writes: the list under `packages`, beside Composer's own `__root__`.
const COMPOSER_2_RECORD: &str = indoc! {r#"
    {
        "packages": [
            {"name": "__root__"},
            {"name": "vendor/a"}
        ],
        "dev": true
    }
"#};

/// The record Composer 1 wrote: the bare list.
const COMPOSER_1_RECORD: &str = indoc! {r#"
    [
        {"name": "vendor/a"}
    ]
"#};

/// A record naming paths that would leave their own directory once joined onto `vendor/`.
const UNSAFE_RECORD: &str = indoc! {r#"
    {
        "packages": [
            {"name": "__root__"},
            {"name": "../src"},
            {"name": "vendor/../../etc"},
            {"name": "vendor/a"}
        ]
    }
"#};

/// A lock with no packages of either kind and no root autoload rules.
fn lock() -> PhastLock {
    PhastLock {
        version: 1,
        content_hash: "test".to_owned(),
        metadata: MetadataSection {
            generated_by: "test".to_owned(),
            generated_at: "2026-01-01T00:00:00Z".to_owned(),
        },
        packages: vec![],
        packages_dev: vec![],
        autoload: Default::default(),
        autoload_dev: Default::default(),
    }
}

/// A `library` package `name` at `version`, carrying no dist, source or extra.
fn package(name: &str, version: &str) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: version.to_owned(),
        package_type: Some("library".to_owned()),
        description: Some(format!("{} description", name)),
        ..Default::default()
    }
}

/// A git source at `reference`, for a package installed from VCS rather than a dist.
fn source(reference: &str) -> SourceRef {
    SourceRef {
        vcs_type: VcsType::Git,
        url: "https://github.com/acme/pkg.git".to_owned(),
        reference: reference.to_owned(),
    }
}

/// The `installed.json` package entry named `name`, panicking when there is none.
///
/// The root is an entry like any other, under the name `__root__`.
fn entry<'a>(json: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    json["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("no installed.json entry for {name}"))
}

// ---------------------------------------------------------------------------
// installed.php: the generated file
// ---------------------------------------------------------------------------

#[test]
fn writes_installed_php() {
    let tmp = TempDir::new().unwrap();
    Installed::new(tmp.path(), false).write(&lock()).unwrap();
    assert!(tmp.path().join("phast/installed.php").exists());
}

#[test]
fn installed_php_returns_an_array() {
    let tmp = TempDir::new().unwrap();
    Installed::new(tmp.path(), false).write(&lock()).unwrap();
    let content = read_installed_php(&tmp);
    assert!(content.starts_with("<?php"));
    assert!(content.contains("return array("));
    assert!(content.trim_end().ends_with(");"));
}

#[test]
fn root_entry_is_present() {
    let tmp = TempDir::new().unwrap();

    Installed::new(tmp.path(), false).write(&lock()).unwrap();

    let content = read_installed_php(&tmp);
    assert!(content.contains("'name' => '__root__'"));
    assert!(content.contains("'install_path'"));
}

// ---------------------------------------------------------------------------
// installed.php: which packages are listed
// ---------------------------------------------------------------------------

#[test]
fn production_packages_are_always_listed() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    lock.packages.push(package("acme/foo", "1.2.3"));

    Installed::new(tmp.path(), false).write(&lock).unwrap();

    let content = read_installed_php(&tmp);
    assert!(content.contains("'acme/foo'"));
    assert!(content.contains("'1.2.3'"));
}

#[test]
fn dev_packages_are_excluded_without_include_dev() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    lock.packages_dev.push(package("dev/tool", "2.0.0"));

    Installed::new(tmp.path(), false).write(&lock).unwrap();

    let content = read_installed_php(&tmp);
    assert!(!content.contains("'dev/tool'"));
}

#[test]
fn dev_packages_are_included_with_include_dev() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    lock.packages_dev.push(package("dev/tool", "2.0.0"));

    Installed::new(tmp.path(), true).write(&lock).unwrap();

    let content = read_installed_php(&tmp);
    assert!(content.contains("'dev/tool'"));
    assert!(content.contains("'dev' => true"));
}

// ---------------------------------------------------------------------------
// installed.php: entry fields
// ---------------------------------------------------------------------------

#[test]
fn install_path_points_at_the_package_directory() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    lock.packages.push(package("acme/foo", "1.0.0"));

    Installed::new(tmp.path(), false).write(&lock).unwrap();

    // Asserting the whole line, because the package name also appears in
    // the `name` field and would pass even with no install_path at all.
    let content = read_installed_php(&tmp);
    assert!(
        content.contains("'install_path' => __DIR__ . '/../acme/foo/'"),
        "install_path must resolve from vendor/phast/:\n{content}",
    );
}

#[test]
fn install_paths_have_trailing_slash() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    lock.packages.push(package("acme/foo", "1.0.0"));

    Installed::new(tmp.path(), false).write(&lock).unwrap();

    let content = read_installed_php(&tmp);
    let paths: Vec<&str> = content
        .lines()
        .filter(|line| line.contains("'install_path'"))
        .collect();

    // The root entry plus the package. Counted, because a loop over no matching
    // lines passes without checking anything.
    assert_eq!(paths.len(), 2, "expected a root and a package install_path");
    for line in paths {
        assert!(
            line.trim_end().ends_with("/',"),
            "install_path must end with /: {line}",
        );
    }
}

#[test]
fn a_missing_package_type_defaults_to_library() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    let mut pkg = package("acme/untyped", "1.0.0");
    pkg.package_type = None;
    lock.packages.push(pkg);

    Installed::new(tmp.path(), false).write(&lock).unwrap();

    // Composer treats an absent `type` as `library`, and packages read this field
    // to decide how to handle a dependency.
    let content = read_installed_php(&tmp);
    assert!(content.contains("'type' => 'library'"));
}

#[test]
fn reference_comes_from_dist() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    let mut pkg = package("acme/bar", "2.0.0");
    pkg.dist = Some(DistSection {
        dist_type: DistType::Zip,
        url: "https://example.com/bar.zip".to_owned(),
        reference: Some("abc123def456".to_owned()),
        shasum: None,
    });
    lock.packages.push(pkg);

    Installed::new(tmp.path(), false).write(&lock).unwrap();

    let content = read_installed_php(&tmp);
    assert!(content.contains("'abc123def456'"));
}

#[test]
fn reference_is_null_when_absent() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    lock.packages.push(package("acme/baz", "3.0.0"));

    Installed::new(tmp.path(), false).write(&lock).unwrap();

    let content = read_installed_php(&tmp);
    assert!(content.contains("'reference' => NULL"));
}

#[test]
fn reference_falls_back_to_the_source() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    let mut pkg = package("acme/from-source", "1.0.0");
    pkg.source = Some(source("srcref999"));
    lock.packages.push(pkg);

    Installed::new(tmp.path(), false).write(&lock).unwrap();

    // A package installed from VCS carries no dist, so the commit it was checked
    // out at is the only reference there is.
    let content = read_installed_php(&tmp);
    assert!(content.contains("'reference' => 'srcref999'"));
}

#[test]
fn a_dist_reference_wins_over_the_source() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    let mut pkg = package("acme/both", "1.0.0");
    pkg.dist = Some(DistSection {
        dist_type: DistType::Zip,
        url: "https://example.com/both.zip".to_owned(),
        reference: Some("distref111".to_owned()),
        shasum: None,
    });
    pkg.source = Some(source("srcref222"));
    lock.packages.push(pkg);

    Installed::new(tmp.path(), false).write(&lock).unwrap();

    // The dist is what was actually installed, so it decides.
    let content = read_installed_php(&tmp);
    assert!(content.contains("'distref111'"));
    assert!(!content.contains("srcref222"));
}

#[test]
fn dev_package_has_dev_requirement_true() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    lock.packages_dev.push(package("dev/tool", "1.0.0"));

    Installed::new(tmp.path(), true).write(&lock).unwrap();

    let content = read_installed_php(&tmp);
    assert!(content.contains("'dev_requirement' => true"));
}

#[test]
fn production_package_has_dev_requirement_false() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    lock.packages.push(package("acme/foo", "1.0.0"));

    Installed::new(tmp.path(), false).write(&lock).unwrap();

    let content = read_installed_php(&tmp);
    assert!(content.contains("'dev_requirement' => false"));
}

// ---------------------------------------------------------------------------
// installed.json
// ---------------------------------------------------------------------------

#[test]
fn json_dev_requirement_is_a_boolean() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    lock.packages.push(package("acme/foo", "1.0.0"));
    lock.packages_dev.push(package("dev/tool", "1.0.0"));

    Installed::new(tmp.path(), true).write(&lock).unwrap();
    let json = read_installed_json(&tmp);

    // A JSON boolean, not the string "false": PHP treats any non-empty string as
    // truthy, so a quoted "false" makes every package look like a dev requirement.
    assert_eq!(
        entry(&json, "acme/foo")["dev_requirement"],
        serde_json::json!(false),
    );
    assert_eq!(
        entry(&json, "dev/tool")["dev_requirement"],
        serde_json::json!(true),
    );
}

#[test]
fn json_uses_composers_install_path_key_and_relative_paths() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    lock.packages.push(package("acme/foo", "1.0.0"));

    Installed::new(tmp.path(), false).write(&lock).unwrap();
    let json = read_installed_json(&tmp);

    // Composer spells it `install-path` here (installed.php uses `install_path`),
    // and states it relative to vendor/composer/ so a vendor tree can be relocated.
    let foo = entry(&json, "acme/foo");
    assert_eq!(foo["install-path"], serde_json::json!("../acme/foo"));
    assert!(
        foo.get("install_path").is_none(),
        "the underscore key belongs to installed.php",
    );

    let root = entry(&json, "__root__");
    assert_eq!(root["install-path"], serde_json::json!("../../"));
}

#[test]
fn json_carries_a_packages_extra_through_unchanged() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    let mut pkg = package("acme/laravel-pkg", "1.0.0");
    pkg.extra = Some(ExtraSection {
        data: serde_json::json!({"laravel": {"providers": ["Acme\\ServiceProvider"]}}),
    });
    lock.packages.push(pkg);

    Installed::new(tmp.path(), false).write(&lock).unwrap();
    let json = read_installed_json(&tmp);

    // Laravel's PackageManifest reads `extra.laravel` from this file to discover
    // service providers, so anything dropped here is a provider never registered.
    assert_eq!(
        entry(&json, "acme/laravel-pkg")["extra"],
        serde_json::json!({"laravel": {"providers": ["Acme\\ServiceProvider"]}}),
    );
}

#[test]
fn json_paths_are_not_machine_specific() {
    let tmp = TempDir::new().unwrap();
    let mut lock = lock();
    lock.packages.push(package("acme/foo", "1.0.0"));

    Installed::new(tmp.path(), false).write(&lock).unwrap();
    let raw = std::fs::read_to_string(tmp.path().join("composer/installed.json")).unwrap();

    // An absolute path would bake the build machine's layout into a generated file,
    // making the output differ per machine and breaking a relocated vendor/.
    assert!(
        !raw.contains(&tmp.path().to_string_lossy().to_string()),
        "installed.json must not embed absolute paths:\n{raw}",
    );
}

#[test]
fn dev_package_names_are_sorted_and_stable() {
    let mut lock = lock();
    for name in [
        "dev/echo",
        "dev/alpha",
        "dev/charlie",
        "dev/bravo",
        "dev/delta",
    ] {
        lock.packages_dev.push(package(name, "1.0.0"));
    }

    let names = |lock: &PhastLock| -> Vec<String> {
        let tmp = TempDir::new().unwrap();
        Installed::new(tmp.path(), true).write(lock).unwrap();
        read_installed_json(&tmp)["dev-package-names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect()
    };

    // The names come from a HashSet, so without an explicit sort this array lands in a
    // different order on every run and an unchanged lock rewrites installed.json.
    assert_eq!(
        names(&lock),
        [
            "dev/alpha",
            "dev/bravo",
            "dev/charlie",
            "dev/delta",
            "dev/echo",
        ],
    );
    assert_eq!(
        names(&lock),
        names(&lock),
        "the same lock must render the same order",
    );
}

fn read_installed_php(tmp: &TempDir) -> String {
    std::fs::read_to_string(tmp.path().join("phast/installed.php")).unwrap()
}

fn read_installed_json(tmp: &TempDir) -> serde_json::Value {
    let raw = std::fs::read_to_string(tmp.path().join("composer/installed.json")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

// ---------------------------------------------------------------------------
// installed.json: reading back what was recorded
// ---------------------------------------------------------------------------

#[test]
fn written_reads_the_names_composer_2_wrote() {
    let tmp = vendor(COMPOSER_2_RECORD);

    assert_eq!(installed::written(tmp.path()), names(&["vendor/a"]));
}

#[test]
fn written_reads_the_bare_array_composer_1_wrote() {
    let tmp = vendor(COMPOSER_1_RECORD);

    assert_eq!(installed::written(tmp.path()), names(&["vendor/a"]));
}

#[test]
fn written_is_empty_without_a_readable_record() {
    let empty = TempDir::new().unwrap();
    assert!(installed::written(empty.path()).is_empty());

    let unreadable = vendor("not json");
    assert!(installed::written(unreadable.path()).is_empty());
}

#[test]
fn written_drops_a_name_that_is_not_vendor_package() {
    let tmp = vendor(UNSAFE_RECORD);

    assert_eq!(installed::written(tmp.path()), names(&["vendor/a"]));
}

/// A `vendor/` directory whose `composer/installed.json` holds `json`.
fn vendor(json: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("composer")).unwrap();
    std::fs::write(tmp.path().join("composer/installed.json"), json).unwrap();
    tmp
}

/// The names as `PackageName`s, the shape `written` returns.
fn names(values: &[&str]) -> Vec<PackageName> {
    values
        .iter()
        .map(|value| PackageName::new(*value))
        .collect()
}
