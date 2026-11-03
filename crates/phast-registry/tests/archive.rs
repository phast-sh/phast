use std::io::{Cursor, Write};

use phast_registry::archive::extractor::extract_zip;
use phast_registry::archive::integrity::verify_sha1;

// Known SHA-1 vectors.
const SHA1_EMPTY: &str = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
const SHA1_ABC: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";

/// An in-memory zip of `(name, contents)` entries, all with default permissions.
fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let options = zip::write::SimpleFileOptions::default();
    let mut buf = Vec::new();
    let mut writer = zip::ZipWriter::new(Cursor::new(&mut buf));
    for (name, contents) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap();
    buf
}

// ---------------------------------------------------------------------------
// Checking an archive against its published checksum
// ---------------------------------------------------------------------------

#[test]
fn matching_checksum_passes() {
    assert!(verify_sha1(b"abc", SHA1_ABC).is_ok());
    assert!(verify_sha1(b"", SHA1_EMPTY).is_ok());
}

#[test]
fn mismatch_returns_actual_hash() {
    // Wrong expected hash: the bytes hash to SHA1_ABC, not this.
    assert_eq!(verify_sha1(b"abc", SHA1_EMPTY), Err(SHA1_ABC.to_owned()));
}

#[test]
fn empty_expected_skips_verification() {
    // No published checksum: anything is accepted (Composer parity).
    assert!(verify_sha1(b"any bytes at all", "").is_ok());
}

// ---------------------------------------------------------------------------
// Extracting a zip into the vendor tree
// ---------------------------------------------------------------------------

#[test]
fn extract_strips_single_root_directory() {
    // Packagist archives wrap everything in a `{vendor}-{package}-{ref}/` dir.
    let zip = archive(&[
        (
            "acme-widget-abc123/composer.json",
            br#"{"name":"acme/widget"}"#,
        ),
        (
            "acme-widget-abc123/src/Widget.php",
            b"<?php class Widget {}",
        ),
    ]);

    let dest = tempfile::tempdir().unwrap();
    extract_zip(&zip, dest.path()).unwrap();

    // The root prefix is stripped: contents land directly under `dest`.
    let composer = std::fs::read_to_string(dest.path().join("composer.json")).unwrap();
    assert_eq!(composer, r#"{"name":"acme/widget"}"#);
    assert!(dest.path().join("src/Widget.php").is_file());
    assert!(
        !dest.path().join("acme-widget-abc123").exists(),
        "the wrapping directory should have been stripped",
    );
}

#[test]
fn extract_rejects_zip_slip() {
    // A traversal entry that would escape `dest` once the prefix is stripped.
    let zip = archive(&[
        ("root/keep.txt", b"safe"),
        ("root/../../escape.txt", b"pwned"),
    ]);

    let dest = tempfile::tempdir().unwrap();
    let err = extract_zip(&zip, dest.path()).expect_err("zip-slip entry must be rejected");
    assert!(
        err.to_string().contains("non-normal path component"),
        "unexpected error: {err}",
    );

    // Nothing escaped above the destination directory.
    assert!(!dest.path().parent().unwrap().join("escape.txt").exists());
}

#[test]
fn extract_rejects_an_absolute_entry_path() {
    let zip = archive(&[("root/keep.txt", b"safe"), ("/etc/evil", b"pwned")]);

    // The same guard catches a leading `/` as catches `..`: every component has
    // to be a plain name before the path is joined onto the destination.
    let dest = tempfile::tempdir().unwrap();
    let err = extract_zip(&zip, dest.path()).expect_err("an absolute entry must be rejected");
    assert!(
        err.to_string().contains("non-normal path component"),
        "unexpected error: {err}",
    );
}

#[test]
fn extract_keeps_entries_that_share_no_root() {
    let zip = archive(&[("a/one.php", b"1"), ("b/two.php", b"2")]);

    // Nothing is stripped when there is no common wrapper, so an archive that is
    // not shaped like Packagist's arrives as it stands.
    let dest = tempfile::tempdir().unwrap();
    extract_zip(&zip, dest.path()).unwrap();
    assert!(dest.path().join("a/one.php").is_file());
    assert!(dest.path().join("b/two.php").is_file());
}

#[test]
fn a_single_file_at_the_root_is_not_stripped() {
    let zip = archive(&[("a.php", b"x")]);

    // The prefix search takes the first entry's leading segment without checking
    // it names a directory, so this yields `a.php/`, which matches no entry and
    // strips nothing. The file lands where it should either way.
    let dest = tempfile::tempdir().unwrap();
    extract_zip(&zip, dest.path()).unwrap();
    assert!(dest.path().join("a.php").is_file());
}

#[test]
fn extract_errors_on_bytes_that_are_not_a_zip() {
    let dest = tempfile::tempdir().unwrap();
    let err = extract_zip(b"not a zip", dest.path()).unwrap_err();
    assert!(
        err.to_string().contains("failed to open zip archive"),
        "{err}",
    );
}

#[cfg(unix)]
#[test]
fn entry_permissions_are_not_carried_over() {
    use std::os::unix::fs::PermissionsExt;

    // Built here rather than with `archive`, which is the only test that needs
    // non-default permissions on an entry.
    let options = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
    let mut buf = Vec::new();
    let mut writer = zip::ZipWriter::new(Cursor::new(&mut buf));
    writer.start_file("root/run.sh", options).unwrap();
    writer.write_all(b"#!/bin/sh\n").unwrap();
    writer.finish().unwrap();

    let dest = tempfile::tempdir().unwrap();
    extract_zip(&buf, dest.path()).unwrap();

    // The executable bit is lost, which is why `vendor/bin` stubs set it again
    // rather than trusting what the archive stored.
    let mode = std::fs::metadata(dest.path().join("run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o644);
}
