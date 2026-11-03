#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use phast_autoload::bin::Bin;
use phast_core::lock::phast::{MetadataSection, PackageDef, PhastLock};

use tempfile::TempDir;

/// A lock holding `packages` as its production entries and no dev ones.
fn lock(packages: Vec<PackageDef>) -> PhastLock {
    PhastLock {
        version: 1,
        content_hash: "test".to_owned(),
        metadata: MetadataSection {
            generated_by: "test".to_owned(),
            generated_at: "2026-01-01T00:00:00Z".to_owned(),
        },
        packages,
        packages_dev: vec![],
        autoload: Default::default(),
        autoload_dev: Default::default(),
    }
}

/// A lock holding `packages` as its dev entries and no production ones.
fn dev_lock(packages: Vec<PackageDef>) -> PhastLock {
    PhastLock {
        packages_dev: packages,
        ..lock(vec![])
    }
}

/// A package `name` at 1.0.0 declaring `bins` as its executables.
fn package(name: &str, bins: &[&str]) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        bin: bins.iter().map(|b| b.to_string()).collect(),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Stub creation
// ---------------------------------------------------------------------------

#[test]
fn creates_relative_symlink_for_declared_binary() {
    let vendor = TempDir::new().unwrap();
    let phast_lock = lock(vec![package("phpunit/phpunit", &["phpunit"])]);

    Bin::new(vendor.path()).generate(&phast_lock, true).unwrap();

    let stub = vendor.path().join("bin/phpunit");
    assert!(stub.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(
        fs::read_link(&stub).unwrap(),
        Path::new("../phpunit/phpunit/phpunit"),
    );
}

#[test]
fn a_bin_path_is_linked_by_its_file_name() {
    let vendor = TempDir::new().unwrap();
    let phast_lock = lock(vec![package("acme/tool", &["bin/tool"])]);

    Bin::new(vendor.path()).generate(&phast_lock, true).unwrap();

    // The stub takes only the file name, while the target keeps the whole declared
    // path, so `bin/tool` is reachable as `vendor/bin/tool` rather than nested again.
    assert_eq!(
        fs::read_link(vendor.path().join("bin/tool")).unwrap(),
        Path::new("../acme/tool/bin/tool"),
    );
}

#[test]
fn the_executable_bit_is_restored_on_the_target() {
    let vendor = TempDir::new().unwrap();
    let target = vendor.path().join("acme/tool/tool");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();

    let phast_lock = lock(vec![package("acme/tool", &["tool"])]);
    Bin::new(vendor.path()).generate(&phast_lock, true).unwrap();

    // Some archives arrive without the bit set, which would leave the stub pointing
    // at something the shell refuses to run.
    let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755);
}

// ---------------------------------------------------------------------------
// The bin directory
// ---------------------------------------------------------------------------

#[test]
fn no_binaries_leaves_no_bin_dir() {
    let vendor = TempDir::new().unwrap();
    let phast_lock = lock(vec![package("acme/lib", &[])]);

    Bin::new(vendor.path()).generate(&phast_lock, true).unwrap();

    // A project with no declared binaries must not be left an empty vendor/bin/.
    assert!(!vendor.path().join("bin").exists());
}

#[test]
fn prunes_bin_dir_when_last_binary_is_removed() {
    let vendor = TempDir::new().unwrap();
    let phast_lock = lock(vec![package("acme/tool", &["tool"])]);

    // First run installs a binary, creating vendor/bin/. The stub is a symlink
    // whose target need not exist, so check the link itself, not what it resolves to.
    Bin::new(vendor.path()).generate(&phast_lock, true).unwrap();
    assert!(vendor.path().join("bin/tool").symlink_metadata().is_ok());

    // Second run has no binaries: the stub is removed and the now-empty dir pruned.
    Bin::new(vendor.path())
        .generate(&lock(vec![]), true)
        .unwrap();
    assert!(!vendor.path().join("bin").exists());
}

#[test]
fn a_user_file_keeps_the_bin_dir_from_being_pruned() {
    let vendor = TempDir::new().unwrap();
    fs::create_dir_all(vendor.path().join("bin")).unwrap();
    fs::write(vendor.path().join("bin/my-script"), "#!/bin/sh\n").unwrap();

    Bin::new(vendor.path())
        .generate(&lock(vec![]), true)
        .unwrap();

    // Pruning only removes an empty directory, so a project with no binaries at all
    // still keeps a vendor/bin/ that holds something we did not put there.
    assert!(vendor.path().join("bin/my-script").exists());
}

// ---------------------------------------------------------------------------
// Dev binaries
// ---------------------------------------------------------------------------

#[test]
fn dev_binaries_are_linked_with_include_dev() {
    let vendor = TempDir::new().unwrap();
    let phast_lock = dev_lock(vec![package("acme/dev-tool", &["devtool"])]);

    Bin::new(vendor.path()).generate(&phast_lock, true).unwrap();

    assert!(vendor.path().join("bin/devtool").symlink_metadata().is_ok());
}

#[test]
fn dev_binaries_are_skipped_without_include_dev() {
    let vendor = TempDir::new().unwrap();
    let phast_lock = dev_lock(vec![package("acme/dev-tool", &["devtool"])]);

    Bin::new(vendor.path())
        .generate(&phast_lock, false)
        .unwrap();

    // The dev package was the only one declaring a binary, so skipping it leaves
    // nothing to install and no vendor/bin/ at all.
    assert!(!vendor.path().join("bin").exists());
}

// ---------------------------------------------------------------------------
// User files
// ---------------------------------------------------------------------------

#[test]
fn a_colliding_user_file_is_replaced_by_the_stub() {
    let vendor = TempDir::new().unwrap();
    fs::create_dir_all(vendor.path().join("bin")).unwrap();
    fs::write(vendor.path().join("bin/phpunit"), "#!/bin/sh\necho mine\n").unwrap();

    let phast_lock = lock(vec![package("phpunit/phpunit", &["phpunit"])]);
    Bin::new(vendor.path()).generate(&phast_lock, true).unwrap();

    // The stale sweep spares a regular file, but a package claiming that name still
    // takes it: vendor/bin/ is generated, not a place to keep your own scripts.
    let stub = vendor.path().join("bin/phpunit");
    assert!(stub.symlink_metadata().unwrap().file_type().is_symlink());
}

#[test]
fn an_unrelated_user_file_is_left_alone() {
    let vendor = TempDir::new().unwrap();
    fs::create_dir_all(vendor.path().join("bin")).unwrap();
    fs::write(vendor.path().join("bin/my-script"), "#!/bin/sh\n").unwrap();

    let phast_lock = lock(vec![package("phpunit/phpunit", &["phpunit"])]);
    Bin::new(vendor.path()).generate(&phast_lock, true).unwrap();

    // No package declares `my-script`, so the stale sweep leaves it: only symlinks
    // pointing into the vendor tree are ours to remove.
    let mine = vendor.path().join("bin/my-script");
    assert!(mine.symlink_metadata().unwrap().file_type().is_file());
}

#[test]
fn a_symlink_outside_the_vendor_tree_is_left_alone() {
    let vendor = TempDir::new().unwrap();
    fs::create_dir_all(vendor.path().join("bin")).unwrap();
    std::os::unix::fs::symlink("/usr/bin/env", vendor.path().join("bin/env-link")).unwrap();

    let phast_lock = lock(vec![package("phpunit/phpunit", &["phpunit"])]);
    Bin::new(vendor.path()).generate(&phast_lock, true).unwrap();

    // Being a symlink is not enough to be ours. The stale sweep claims only targets
    // that are relative or sit inside vendor/, so a link elsewhere survives.
    assert!(
        vendor
            .path()
            .join("bin/env-link")
            .symlink_metadata()
            .is_ok(),
    );
}
