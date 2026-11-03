#![cfg(unix)]

use std::fs;
use std::path::Path;

use phast_cli::installer::path_linker::PathLinker;
use phast_core::lock::phast::{DistSection, DistType, PackageDef};

/// A path package named `name` whose source lives at `source`, relative to the
/// project root rather than to `vendor/`.
fn package(name: &str, source: &str) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        dist: Some(DistSection {
            dist_type: DistType::Path,
            url: source.to_owned(),
            reference: None,
            shasum: None,
        }),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// The symlink a package gets
// ---------------------------------------------------------------------------

#[test]
fn links_package_to_relative_source() {
    let root = tempfile::tempdir().unwrap();
    // Source tree at <root>/packages/local with a marker file.
    let source = root.path().join("packages/local");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("marker.txt"), "hello").unwrap();

    let vendor = root.path().join("vendor");
    PathLinker::new(&vendor)
        .link(&[package("acme/local", "packages/local")])
        .unwrap();

    let link = vendor.join("acme/local");
    // The entry is a symlink with the expected relative target.
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(
        fs::read_link(&link).unwrap(),
        Path::new("../../packages/local"),
    );
    // It resolves to the source: the marker is readable through it.
    assert_eq!(
        fs::read_to_string(link.join("marker.txt")).unwrap(),
        "hello",
    );
}

#[test]
fn every_package_in_one_call_is_linked() {
    let root = tempfile::tempdir().unwrap();
    for source in ["libs/a", "libs/b"] {
        fs::create_dir_all(root.path().join(source)).unwrap();
    }

    let vendor = root.path().join("vendor");
    PathLinker::new(&vendor)
        .link(&[package("acme/a", "libs/a"), package("other/b", "libs/b")])
        .unwrap();

    // Each lands under its own vendor directory, which the linker creates.
    assert_eq!(
        fs::read_link(vendor.join("acme/a")).unwrap(),
        Path::new("../../libs/a"),
    );
    assert_eq!(
        fs::read_link(vendor.join("other/b")).unwrap(),
        Path::new("../../libs/b"),
    );
}

#[test]
fn a_source_outside_the_project_root_still_links() {
    let root = tempfile::tempdir().unwrap();
    let vendor = root.path().join("vendor");

    PathLinker::new(&vendor)
        .link(&[package("acme/out", "../sibling")])
        .unwrap();

    // The `../..` prefix is prepended whatever the source looks like, so a
    // source above the project root simply climbs one further.
    assert_eq!(
        fs::read_link(vendor.join("acme/out")).unwrap(),
        Path::new("../../../sibling"),
    );
}

#[test]
fn a_package_without_a_dist_is_an_error() {
    let root = tempfile::tempdir().unwrap();
    let bare = PackageDef {
        name: "acme/bare".to_owned(),
        version: "1.0.0".to_owned(),
        ..Default::default()
    };

    let err = PathLinker::new(&root.path().join("vendor"))
        .link(&[bare])
        .unwrap_err();

    assert!(err.to_string().contains("no source directory"), "{err}");
}

// ---------------------------------------------------------------------------
// Replacing whatever already sits at vendor/<name>
// ---------------------------------------------------------------------------

#[test]
fn relinking_retargets_an_existing_link() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("libs/one")).unwrap();
    fs::create_dir_all(root.path().join("libs/two")).unwrap();

    let vendor = root.path().join("vendor");
    let linker = PathLinker::new(&vendor);
    linker.link(&[package("acme/local", "libs/one")]).unwrap();
    linker.link(&[package("acme/local", "libs/two")]).unwrap();

    // `update` relinks a package whose source moved, so the second call has to
    // repoint the link rather than merely leave a working one in place.
    assert_eq!(
        fs::read_link(vendor.join("acme/local")).unwrap(),
        Path::new("../../libs/two"),
    );
}

#[test]
fn a_plain_file_in_the_way_is_replaced() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("packages/local")).unwrap();

    let vendor = root.path().join("vendor");
    fs::create_dir_all(vendor.join("acme")).unwrap();
    fs::write(vendor.join("acme/local"), "not a package").unwrap();

    PathLinker::new(&vendor)
        .link(&[package("acme/local", "packages/local")])
        .unwrap();

    let link = vendor.join("acme/local");
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
}

#[test]
fn a_real_directory_in_the_way_is_an_error() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("packages/local")).unwrap();

    let vendor = root.path().join("vendor");
    fs::create_dir_all(vendor.join("acme/local")).unwrap();
    fs::write(vendor.join("acme/local/old.php"), "from an archive").unwrap();

    let err = PathLinker::new(&vendor)
        .link(&[package("acme/local", "packages/local")])
        .unwrap_err();

    // A known limitation, not a design choice: the entry is removed with
    // `remove_file`, which refuses a directory. It is reachable when a package
    // installed from an archive later becomes a `path` package.
    assert!(
        err.to_string().contains("failed to remove existing"),
        "{err}",
    );
}
