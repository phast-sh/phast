use std::collections::BTreeMap;
use std::path::PathBuf;

use phast_autoload::builder::Builder;
use phast_core::lock::phast::{AutoloadSection, MetadataSection, PackageDef, PhastLock};
use phast_workspace::MemberAutoload;

use tempfile::TempDir;

/// A lock with no packages and no root autoload rules, for a test to fill in.
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

/// A package `name` at 1.0.0 declaring `autoload` and nothing under `autoload-dev`.
fn package(name: &str, autoload: AutoloadSection) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        autoload,
        ..Default::default()
    }
}

/// A workspace member rooted at `dir`, declaring `autoload` and `autoload_dev`.
fn member(
    dir: PathBuf,
    autoload: AutoloadSection,
    autoload_dev: AutoloadSection,
) -> MemberAutoload {
    MemberAutoload {
        dir,
        autoload,
        autoload_dev,
    }
}

/// One PSR-4 `prefix` mapped to `dir`, with no other autoload rules.
fn psr4(prefix: &str, dir: &str) -> AutoloadSection {
    AutoloadSection {
        psr4: BTreeMap::from([(prefix.to_owned(), vec![dir.to_owned()])]),
        ..Default::default()
    }
}

/// The `files` autoload rule listing `paths`, with no other autoload rules.
fn files(paths: &[&str]) -> AutoloadSection {
    AutoloadSection {
        files: paths.iter().map(|p| (*p).to_owned()).collect(),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Root package autoload
// ---------------------------------------------------------------------------

#[test]
fn root_psr4_entries_are_collected() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path().join("vendor");
    std::fs::create_dir_all(&vendor).unwrap();

    let mut lock = lock();
    lock.autoload = AutoloadSection {
        psr4: BTreeMap::from([("App\\".to_owned(), vec!["app/".to_owned()])]),
        ..Default::default()
    };

    let maps = Builder::new(&vendor, false).build(&lock, &[]).unwrap();
    let dirs = maps
        .psr4
        .get("App\\")
        .expect("root PSR-4 prefix must be present");
    assert_eq!(dirs.len(), 1);
    assert_eq!(dirs[0], "../app");
}

#[test]
fn root_psr0_entries_are_collected() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path().join("vendor");
    std::fs::create_dir_all(&vendor).unwrap();

    let mut lock = lock();
    lock.autoload = AutoloadSection {
        psr0: BTreeMap::from([("Legacy_".to_owned(), vec!["src/".to_owned()])]),
        ..Default::default()
    };

    let maps = Builder::new(&vendor, false).build(&lock, &[]).unwrap();
    let dirs = maps
        .psr0
        .get("Legacy_")
        .expect("root PSR-0 prefix must be present");
    assert_eq!(dirs.len(), 1);
    assert_eq!(dirs[0], "../src");
}

#[test]
fn root_files_entries_are_collected() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path().join("vendor");
    std::fs::create_dir_all(&vendor).unwrap();

    let mut lock = lock();
    lock.autoload = AutoloadSection {
        files: vec!["bootstrap/helpers.php".to_owned()],
        ..Default::default()
    };

    let maps = Builder::new(&vendor, false).build(&lock, &[]).unwrap();
    assert_eq!(maps.files.len(), 1);
    let path = maps.files.values().next().unwrap();
    assert_eq!(path, "../bootstrap/helpers.php");
}

#[test]
fn root_dev_autoload_is_excluded_without_include_dev() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path().join("vendor");
    std::fs::create_dir_all(&vendor).unwrap();

    let mut lock = lock();
    lock.autoload_dev = AutoloadSection {
        psr4: BTreeMap::from([("Tests\\".to_owned(), vec!["tests/".to_owned()])]),
        files: vec!["tests/bootstrap.php".to_owned()],
        ..Default::default()
    };

    let maps = Builder::new(&vendor, false).build(&lock, &[]).unwrap();
    assert!(
        maps.psr4.is_empty(),
        "root dev PSR-4 must not appear when include_dev=false",
    );
    assert!(
        maps.files.is_empty(),
        "root dev files must not appear when include_dev=false",
    );
}

#[test]
fn root_dev_autoload_is_included_with_include_dev() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path().join("vendor");
    std::fs::create_dir_all(&vendor).unwrap();

    let mut lock = lock();
    lock.autoload_dev = AutoloadSection {
        psr4: BTreeMap::from([("Tests\\".to_owned(), vec!["tests/".to_owned()])]),
        files: vec!["tests/bootstrap.php".to_owned()],
        ..Default::default()
    };

    let maps = Builder::new(&vendor, true).build(&lock, &[]).unwrap();
    let dirs = maps
        .psr4
        .get("Tests\\")
        .expect("root dev PSR-4 must appear when include_dev=true");
    assert_eq!(dirs[0], "../tests");
    assert_eq!(maps.files.len(), 1);
    assert_eq!(
        maps.files.values().next().unwrap(),
        "../tests/bootstrap.php",
    );
}

// ---------------------------------------------------------------------------
// PSR-4
// ---------------------------------------------------------------------------

#[test]
fn psr4_entries_are_collected() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path();
    std::fs::create_dir_all(vendor.join("acme/foo")).unwrap();

    let mut lock = lock();
    lock.packages.push(package(
        "acme/foo",
        AutoloadSection {
            psr4: BTreeMap::from([("Acme\\Foo\\".to_owned(), vec!["src/".to_owned()])]),
            ..Default::default()
        },
    ));

    let maps = Builder::new(vendor, false).build(&lock, &[]).unwrap();
    let dirs = maps
        .psr4
        .get("Acme\\Foo\\")
        .expect("prefix must be present");
    assert_eq!(dirs.len(), 1);
    assert_eq!(dirs[0], "acme/foo/src");
}

#[test]
fn psr4_same_prefix_from_two_packages_merges() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path();
    std::fs::create_dir_all(vendor.join("pkg/a")).unwrap();
    std::fs::create_dir_all(vendor.join("pkg/b")).unwrap();

    let mut lock = lock();
    lock.packages.push(package(
        "pkg/a",
        AutoloadSection {
            psr4: BTreeMap::from([("Shared\\".to_owned(), vec!["src/".to_owned()])]),
            ..Default::default()
        },
    ));
    lock.packages.push(package(
        "pkg/b",
        AutoloadSection {
            psr4: BTreeMap::from([("Shared\\".to_owned(), vec!["lib/".to_owned()])]),
            ..Default::default()
        },
    ));

    let maps = Builder::new(vendor, false).build(&lock, &[]).unwrap();
    let dirs = maps.psr4.get("Shared\\").expect("prefix must be present");
    assert_eq!(dirs.len(), 2, "both packages should contribute a directory");

    // PHP tries the directories in order and takes the first hit, so which
    // package wins a shared prefix is decided by lock order, not chance.
    assert_eq!(dirs, &["pkg/a/src", "pkg/b/lib"]);
}

// ---------------------------------------------------------------------------
// What include_dev gates: lock dev packages, and a member's dev rules
// ---------------------------------------------------------------------------

#[test]
fn dev_packages_are_excluded_without_include_dev() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path();
    std::fs::create_dir_all(vendor.join("dev/pkg")).unwrap();

    let mut lock = lock();
    lock.packages_dev.push(package(
        "dev/pkg",
        AutoloadSection {
            psr4: BTreeMap::from([("Dev\\".to_owned(), vec!["src/".to_owned()])]),
            ..Default::default()
        },
    ));

    let maps = Builder::new(vendor, false).build(&lock, &[]).unwrap();
    assert!(
        maps.psr4.is_empty(),
        "dev PSR-4 must not appear when include_dev=false",
    );
}

#[test]
fn dev_packages_are_included_with_include_dev() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path();
    std::fs::create_dir_all(vendor.join("dev/pkg")).unwrap();

    let mut lock = lock();
    lock.packages_dev.push(package(
        "dev/pkg",
        AutoloadSection {
            psr4: BTreeMap::from([("Dev\\".to_owned(), vec!["src/".to_owned()])]),
            ..Default::default()
        },
    ));

    let maps = Builder::new(vendor, true).build(&lock, &[]).unwrap();
    assert!(
        maps.psr4.contains_key("Dev\\"),
        "dev PSR-4 must appear when include_dev=true",
    );
}

#[test]
fn member_dev_autoload_is_excluded_without_include_dev() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path().join("vendor");
    std::fs::create_dir_all(&vendor).unwrap();

    let logger = member(
        PathBuf::from("libs/logger"),
        psr4("Logger\\", "src/"),
        psr4("Logger\\Tests\\", "tests/"),
    );
    let maps = Builder::new(&vendor, false)
        .build(&lock(), std::slice::from_ref(&logger))
        .unwrap();

    assert!(maps.psr4.contains_key("Logger\\"));
    assert!(
        !maps.psr4.contains_key("Logger\\Tests\\"),
        "member dev PSR-4 must not appear when include_dev=false",
    );
}

#[test]
fn member_dev_autoload_is_included_with_include_dev() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path().join("vendor");
    std::fs::create_dir_all(&vendor).unwrap();

    let logger = member(
        PathBuf::from("libs/logger"),
        psr4("Logger\\", "src/"),
        psr4("Logger\\Tests\\", "tests/"),
    );
    let maps = Builder::new(&vendor, true)
        .build(&lock(), std::slice::from_ref(&logger))
        .unwrap();

    assert!(
        maps.psr4.contains_key("Logger\\Tests\\"),
        "member dev PSR-4 must appear when include_dev=true",
    );
}

// ---------------------------------------------------------------------------
// Files entries, and the order they are required in
// ---------------------------------------------------------------------------

#[test]
fn files_entries_are_collected() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path();
    std::fs::create_dir_all(vendor.join("acme/helpers")).unwrap();

    let mut lock = lock();
    lock.packages.push(package(
        "acme/helpers",
        AutoloadSection {
            files: vec!["src/helpers.php".to_owned()],
            ..Default::default()
        },
    ));

    let maps = Builder::new(vendor, false).build(&lock, &[]).unwrap();
    assert_eq!(maps.files.len(), 1);
    let path = maps.files.values().next().unwrap();
    assert!(path.ends_with("acme/helpers/src/helpers.php"));
}

#[test]
fn the_same_file_from_two_packages_keeps_both_entries() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path();
    std::fs::create_dir_all(vendor.join("pkg/a")).unwrap();
    std::fs::create_dir_all(vendor.join("pkg/b")).unwrap();

    // Both declare `bootstrap.php`, but each resolves under its own package
    // directory, so the two absolute paths differ and neither key collides.
    // Nothing is deduplicated here: both files have to be required.
    let mut lock = lock();
    lock.packages.push(package(
        "pkg/a",
        AutoloadSection {
            files: vec!["bootstrap.php".to_owned()],
            ..Default::default()
        },
    ));
    lock.packages.push(package(
        "pkg/b",
        AutoloadSection {
            files: vec!["bootstrap.php".to_owned()],
            ..Default::default()
        },
    ));

    let maps = Builder::new(vendor, false).build(&lock, &[]).unwrap();
    assert_eq!(
        maps.files.len(),
        2,
        "different packages produce different file keys",
    );
}

#[test]
fn vendor_files_precede_root_files() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path().join("vendor");
    std::fs::create_dir_all(vendor.join("acme/helpers")).unwrap();

    let mut lock = lock();
    lock.autoload = AutoloadSection {
        files: vec!["bootstrap/autoload.php".to_owned()],
        ..Default::default()
    };
    lock.packages.push(package(
        "acme/helpers",
        AutoloadSection {
            files: vec!["src/helpers.php".to_owned()],
            ..Default::default()
        },
    ));

    // The root depends on its packages, so its files load last: vendor package
    // files come first, the root file (a `../` path) last.
    let maps = Builder::new(&vendor, false).build(&lock, &[]).unwrap();
    let paths: Vec<&str> = maps.files.values().map(|s| s.as_str()).collect();
    assert_eq!(paths.len(), 2);
    assert!(
        !paths[0].starts_with("../"),
        "vendor file must be first, got: {:?}",
        paths[0],
    );
    assert!(
        paths[1].starts_with("../"),
        "root file must be last, got: {:?}",
        paths[1],
    );
}

#[test]
fn member_files_fall_between_vendor_and_root() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path().join("vendor");
    std::fs::create_dir_all(vendor.join("acme/helpers")).unwrap();

    let mut lock = lock();
    lock.autoload = AutoloadSection {
        files: vec!["bootstrap.php".to_owned()],
        ..Default::default()
    };
    lock.packages.push(package(
        "acme/helpers",
        AutoloadSection {
            files: vec!["src/helpers.php".to_owned()],
            ..Default::default()
        },
    ));

    // A workspace member depends on the external packages and is depended on by
    // the root, so its files land after vendor packages and before the root.
    let logger = member(
        tmp.path().join("libs/logger"),
        files(&["boot.php"]),
        AutoloadSection::default(),
    );

    let maps = Builder::new(&vendor, false)
        .build(&lock, std::slice::from_ref(&logger))
        .unwrap();
    let paths: Vec<&str> = maps.files.values().map(|s| s.as_str()).collect();

    assert_eq!(paths.len(), 3, "got: {paths:?}");
    assert!(paths[0].contains("acme/helpers"), "vendor first: {paths:?}");
    assert!(paths[1].contains("libs/logger"), "member second: {paths:?}");
    assert!(paths[2].contains("bootstrap.php"), "root last: {paths:?}");
}

// ---------------------------------------------------------------------------
// Classmap exclusion
// ---------------------------------------------------------------------------

#[test]
fn excluded_dir_kept_out_of_psr4_optimized_classmap() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path();
    let pkg = vendor.join("acme/contracts");

    // A real class and a Test/ helper both live under the PSR-4 root.
    write_class(&pkg.join("src/Service.php"), "Acme\\Contracts", "Service");
    write_class(
        &pkg.join("src/Test/Fixture.php"),
        "Acme\\Contracts\\Test",
        "Fixture",
    );

    let mut lock = lock();
    lock.packages.push(package(
        "acme/contracts",
        AutoloadSection {
            psr4: BTreeMap::from([("Acme\\Contracts\\".to_owned(), vec!["src/".to_owned()])]),
            exclude: vec!["/src/Test/".to_owned()],
            ..Default::default()
        },
    ));

    let maps = Builder::new(vendor, false).build(&lock, &[]).unwrap();

    assert!(
        maps.classmap.contains_key("Acme\\Contracts\\Service"),
        "non-excluded class must be in the optimized classmap",
    );
    assert!(
        !maps.classmap.contains_key("Acme\\Contracts\\Test\\Fixture"),
        "class under an excluded directory must not be in the classmap",
    );
}

#[test]
fn excluded_path_kept_out_of_declared_classmap_scan() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path();
    let pkg = vendor.join("acme/lib");

    write_class(&pkg.join("lib/Real.php"), "Acme\\Lib", "Real");
    write_class(
        &pkg.join("lib/fixtures/Sample.php"),
        "Acme\\Lib\\Fixtures",
        "Sample",
    );

    let mut lock = lock();
    lock.packages.push(package(
        "acme/lib",
        AutoloadSection {
            classmap: vec!["lib/".to_owned()],
            exclude: vec!["lib/fixtures/".to_owned()],
            ..Default::default()
        },
    ));

    let maps = Builder::new(vendor, false).build(&lock, &[]).unwrap();

    assert!(
        maps.classmap.contains_key("Acme\\Lib\\Real"),
        "non-excluded class must be scanned into the classmap",
    );
    assert!(
        !maps.classmap.contains_key("Acme\\Lib\\Fixtures\\Sample"),
        "class under an excluded path must not be scanned into the classmap",
    );
}

fn write_class(path: &std::path::Path, ns: &str, class: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        format!("<?php\nnamespace {ns};\nclass {class} {{}}\n"),
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Repeated builds of the same tree
// ---------------------------------------------------------------------------

#[test]
fn the_same_tree_builds_the_same_maps_twice() {
    let tmp = TempDir::new().unwrap();
    let vendor = tmp.path().join("vendor");
    let pkg = vendor.join("acme/lib");
    write_class(&pkg.join("src/One.php"), "Acme\\Lib", "One");
    write_class(&pkg.join("src/Two.php"), "Acme\\Lib", "Two");

    let mut lock = lock();
    lock.autoload = AutoloadSection {
        files: vec!["bootstrap.php".to_owned()],
        ..Default::default()
    };
    lock.packages.push(package(
        "acme/lib",
        AutoloadSection {
            psr4: BTreeMap::from([("Acme\\Lib\\".to_owned(), vec!["src/".to_owned()])]),
            files: vec!["helpers.php".to_owned()],
            ..Default::default()
        },
    ));

    // The PSR roots are scanned in parallel and the maps are keyed by hash, so a
    // second build is the only thing that shows the output does not drift.
    let a = Builder::new(&vendor, false).build(&lock, &[]).unwrap();
    let b = Builder::new(&vendor, false).build(&lock, &[]).unwrap();

    assert_eq!(a.classmap, b.classmap);
    assert_eq!(a.psr4, b.psr4);
    assert_eq!(
        a.files.into_iter().collect::<Vec<_>>(),
        b.files.into_iter().collect::<Vec<_>>(),
    );
}
