use phast_cli::commands::cli::update::Archive;

use sha2::{Digest, Sha256};

/// A gzipped tarball holding `entries`, each at the path it is named with.
fn tarball(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::default(),
    ));
    for (name, contents) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append_data(&mut header, name, *contents).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

/// A sha256sum-format checksum line for `bytes`, as the release publishes it.
fn checksum(bytes: &[u8], name: &str) -> Vec<u8> {
    format!("{}  {name}\n", hex::encode(Sha256::digest(bytes))).into_bytes()
}

// ---------------------------------------------------------------------------
// Verifying the downloaded archive against its published checksum
// ---------------------------------------------------------------------------

#[test]
fn verify_accepts_a_matching_checksum() {
    let bytes = b"the release archive bytes".to_vec();
    let sums = checksum(&bytes, "phast-x86_64-unknown-linux-musl.tar.gz");
    assert!(Archive::new(bytes).verify(&sums).is_ok());
}

#[test]
fn verify_is_case_insensitive() {
    let bytes = b"payload".to_vec();
    let lower = checksum(&bytes, "phast.tar.gz");
    let upper = String::from_utf8(lower)
        .unwrap()
        .to_uppercase()
        .into_bytes();
    assert!(Archive::new(bytes).verify(&upper).is_ok());
}

#[test]
fn verify_rejects_a_mismatch() {
    let sums = checksum(b"a different archive", "phast.tar.gz");
    let err = Archive::new(b"the real archive".to_vec())
        .verify(&sums)
        .unwrap_err();
    assert!(err.to_string().contains("checksum mismatch"), "{err}");
}

#[test]
fn verify_rejects_an_empty_checksum_file() {
    let err = Archive::new(b"anything".to_vec()).verify(b"").unwrap_err();
    assert!(err.to_string().contains("checksum file is empty"), "{err}");
}

#[test]
fn verify_rejects_a_checksum_file_of_only_whitespace() {
    // There is a token to find only if something is not whitespace, so this
    // lands on the same complaint as a zero-byte file rather than on a mismatch.
    let err = Archive::new(b"anything".to_vec())
        .verify(b"  \n \t ")
        .unwrap_err();
    assert!(err.to_string().contains("checksum file is empty"), "{err}");
}

#[test]
fn verify_rejects_a_checksum_file_that_is_not_utf8() {
    let err = Archive::new(b"anything".to_vec())
        .verify(&[0xff, 0xfe, 0x00])
        .unwrap_err();
    assert!(err.to_string().contains("not valid UTF-8"), "{err}");
}

#[test]
fn verify_ignores_the_name_beside_the_hash() {
    let bytes = b"the release archive bytes".to_vec();
    let sums = checksum(&bytes, "some-other-asset.zip");

    // Only the hash is compared. The asset a checksum file claims to describe is
    // never checked against the asset actually downloaded.
    assert!(Archive::new(bytes).verify(&sums).is_ok());
}

#[test]
fn verify_reads_only_the_first_line() {
    let bytes = b"the release archive bytes".to_vec();
    let mut sums = checksum(b"a different asset", "other.tar.gz");
    sums.extend(checksum(&bytes, "phast.tar.gz"));

    // Each asset publishes its own `<asset>.sha256`, so the first token is the
    // whole file. A combined SHA256SUMS listing would fail here, not search.
    let err = Archive::new(bytes).verify(&sums).unwrap_err();
    assert!(err.to_string().contains("checksum mismatch"), "{err}");
}

// ---------------------------------------------------------------------------
// Pulling the binary out of the tarball
// ---------------------------------------------------------------------------

#[test]
fn extract_pulls_the_phast_binary() {
    let archive = Archive::new(tarball(&[("phast", b"#!/bin/echo i am phast")]));
    let binary = archive.extract().unwrap();
    assert_eq!(binary, b"#!/bin/echo i am phast");
}

#[test]
fn extract_finds_the_binary_below_the_archive_root() {
    // Matching is on the entry's file name, not its full path, so a release that
    // nests the binary one directory down still yields it.
    let archive = Archive::new(tarball(&[("bin/phast", b"nested binary")]));
    assert_eq!(archive.extract().unwrap(), b"nested binary");
}

#[test]
fn extract_takes_the_first_matching_entry() {
    let archive = Archive::new(tarball(&[("a/phast", b"first"), ("b/phast", b"second")]));
    assert_eq!(archive.extract().unwrap(), b"first");
}

#[test]
fn extract_errors_when_the_binary_is_absent() {
    let archive = Archive::new(tarball(&[("README.md", b"no binary here")]));
    let err = archive.extract().unwrap_err();
    assert!(err.to_string().contains("did not contain"), "{err}");
}

#[test]
fn extract_errors_on_bytes_that_are_not_an_archive() {
    let archive = Archive::new(b"not a tarball".to_vec());
    let err = archive.extract().unwrap_err();
    assert!(err.to_string().contains("corrupt entry"), "{err}");
}
