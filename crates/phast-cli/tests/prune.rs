use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use phast_cli::installer::prune;
use phast_core::PackageName;
use phast_core::lock::phast::PackageDef;

/// A package `name` at 1.0.0, as an install plan lists it.
fn package(name: &str) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        ..Default::default()
    }
}

/// An empty file at `path`, its directories created; only whether it exists is asserted.
fn file(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "").unwrap();
}

/// The names as `PackageName`s, the shape `installed.json` is read into.
fn names(values: &[&str]) -> Vec<PackageName> {
    values
        .iter()
        .map(|value| PackageName::new(*value))
        .collect()
}

// ---------------------------------------------------------------------------
// Which packages leave vendor/
// ---------------------------------------------------------------------------

#[test]
fn a_package_missing_from_the_plan_is_stale() {
    let written = names(&["vendor/kept", "vendor/dropped"]);

    assert_eq!(
        prune::stale(&written, &[package("vendor/kept")]),
        names(&["vendor/dropped"]),
    );
}

#[test]
fn nothing_is_stale_while_the_plan_keeps_everything() {
    let written = names(&["vendor/a", "vendor/b"]);

    assert!(prune::stale(&written, &[package("vendor/a"), package("vendor/b")]).is_empty());
}

// ---------------------------------------------------------------------------
// Removing from vendor/
// ---------------------------------------------------------------------------

#[test]
fn remove_deletes_the_package_and_its_emptied_vendor_directory() {
    let root = tempfile::tempdir().unwrap();
    let vendor_dir = root.path();
    file(&vendor_dir.join("phpunit/phpunit/src/TestCase.php"));

    let packages = names(&["phpunit/phpunit"]);
    let removed = prune::remove(vendor_dir, &packages).unwrap();

    assert_eq!(removed, 1);
    assert!(!vendor_dir.join("phpunit").exists());
}

#[test]
fn remove_keeps_a_vendor_directory_that_still_holds_a_package() {
    let root = tempfile::tempdir().unwrap();
    let vendor_dir = root.path();
    file(&vendor_dir.join("symfony/console/composer.json"));
    file(&vendor_dir.join("symfony/process/composer.json"));

    let packages = names(&["symfony/console"]);
    prune::remove(vendor_dir, &packages).unwrap();

    assert!(!vendor_dir.join("symfony/console").exists());
    assert!(vendor_dir.join("symfony/process/composer.json").exists());
}

/// A path package is a symlink to someone's source, which has to outlive the link.
#[test]
fn remove_unlinks_a_path_package_without_touching_its_source() {
    let root = tempfile::tempdir().unwrap();
    let vendor_dir = root.path().join("vendor");
    let source = root.path().join("packages/api");

    file(&source.join("composer.json"));
    fs::create_dir_all(vendor_dir.join("acme")).unwrap();
    symlink(&source, vendor_dir.join("acme/api")).unwrap();

    let packages = names(&["acme/api"]);
    prune::remove(&vendor_dir, &packages).unwrap();

    assert!(fs::symlink_metadata(vendor_dir.join("acme/api")).is_err());
    assert!(source.join("composer.json").exists());
}

/// A committed `vendor/` can hold a vendor-name directory that is really a link elsewhere;
/// removing `elsewhere/precious` through it would delete outside `vendor/`.
#[test]
fn remove_skips_a_package_behind_a_symlinked_vendor_directory() {
    let root = tempfile::tempdir().unwrap();
    let vendor_dir = root.path().join("vendor");
    let outside = root.path().join("outside");

    file(&outside.join("precious/file"));
    fs::create_dir_all(&vendor_dir).unwrap();
    symlink(&outside, vendor_dir.join("elsewhere")).unwrap();

    let packages = names(&["elsewhere/precious"]);
    let removed = prune::remove(&vendor_dir, &packages).unwrap();

    assert_eq!(removed, 0);
    assert!(outside.join("precious/file").exists());
}

#[test]
fn remove_skips_a_package_that_is_not_there() {
    let root = tempfile::tempdir().unwrap();
    let vendor_dir = root.path();

    let packages = names(&["vendor/absent"]);
    let removed = prune::remove(vendor_dir, &packages).unwrap();

    assert_eq!(removed, 0);
}
