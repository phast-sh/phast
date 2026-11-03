use phast_registry::p2::expand_versions;

use serde_json::{Value as JsonValue, json};

/// The versions `entries` expand to, named as the registry published them, in the
/// order they came back.
fn expanded(entries: Vec<JsonValue>, minified: bool) -> Vec<String> {
    expand_versions(entries, minified)
        .into_iter()
        .filter_map(|version| version.version)
        .collect()
}

// ---------------------------------------------------------------------------
// A response that is not minified
// ---------------------------------------------------------------------------

#[test]
fn every_entry_is_returned_in_the_order_it_arrived() {
    let entries = vec![
        json!({ "version": "1.1.0", "require": { "php": "^8.1" } }),
        json!({ "version": "1.0.0", "require": { "php": "^8.0" } }),
    ];

    // Packagist sends newest first and the order is left alone: the resolver
    // sorts by the parsed version, not by where an entry sat in the response.
    assert_eq!(expanded(entries, false), ["1.1.0", "1.0.0"]);
}

#[test]
fn nothing_is_inherited_when_the_response_is_not_minified() {
    let entries = vec![
        json!({ "version": "1.1.0", "require": { "php": "^8.1" } }),
        json!({ "version": "1.0.0" }),
    ];

    // The same entries read as diffs under `minified`, so the flag decides
    // whether a version declaring no requirements has none or inherits some.
    let versions = expand_versions(entries, false);
    assert!(versions[1].require.is_empty());
}

#[test]
fn an_empty_response_expands_to_no_versions() {
    assert!(expand_versions(vec![], false).is_empty());
    assert!(expand_versions(vec![], true).is_empty());
}

// ---------------------------------------------------------------------------
// Merging a minified response forward
// ---------------------------------------------------------------------------

#[test]
fn a_delta_inherits_every_field_it_does_not_carry() {
    let entries = vec![
        json!({
            "version": "1.1.0",
            "type": "library",
            "require": { "php": "^8.1" },
            "dist": { "type": "zip", "url": "https://example.com/1.1.0.zip" },
            "source": {
                "type": "git",
                "url": "https://github.com/acme/widget.git",
                "reference": "c2",
            },
            "bin": ["bin/widget"],
        }),
        // Only the version changes, which is the whole point of the format.
        json!({ "version": "1.0.0" }),
    ];

    let versions = expand_versions(entries, true);
    let older = &versions[1];
    assert_eq!(older.version.as_deref(), Some("1.0.0"));
    assert_eq!(older.package_type.as_deref(), Some("library"));
    assert_eq!(older.require.get("php").map(String::as_str), Some("^8.1"));
    assert_eq!(older.bin, ["bin/widget"]);
    assert!(older.dist.is_some());

    let source = older.source.as_ref().expect("source is inherited too");
    assert_eq!(source.reference, "c2");
}

#[test]
fn a_delta_overrides_the_field_it_carries() {
    let entries = vec![
        json!({ "version": "2.0.0", "require": { "php": "^8.2" } }),
        json!({ "version": "1.0.0", "require": { "php": "^8.0" } }),
    ];

    let versions = expand_versions(entries, true);
    assert_eq!(
        versions[1].require.get("php").map(String::as_str),
        Some("^8.0"),
    );
}

#[test]
fn unset_removes_an_inherited_field() {
    let entries = vec![
        json!({
            "version": "2.0.0",
            "require": { "php": "^8.1" },
            "dist": { "type": "zip", "url": "https://example.com/2.0.0.zip" },
        }),
        json!({ "version": "1.0.0", "require": "__unset", "dist": "__unset" }),
    ];

    // Without the sentinel a field could only ever be replaced, so a version
    // that dropped a requirement would keep carrying it forward.
    let versions = expand_versions(entries, true);
    assert!(versions[1].require.is_empty());
    assert!(versions[1].dist.is_none());
}

#[test]
fn unset_on_a_field_that_was_never_set_is_harmless() {
    let entries = vec![json!({ "version": "1.0.0", "dist": "__unset" })];

    // The sentinel removes a key that is not there, so a first entry carrying
    // one is still a version rather than a dropped entry.
    let versions = expand_versions(entries, true);
    assert_eq!(versions.len(), 1);
    assert!(versions[0].dist.is_none());
}

// ---------------------------------------------------------------------------
// An entry the format does not fit
// ---------------------------------------------------------------------------

#[test]
fn an_entry_that_does_not_deserialize_is_dropped() {
    // Every one of these is a shape the type cannot hold, so the version is
    // passed over rather than failing the whole response.
    let malformed = [
        json!({ "version": "1.0.0", "dist": { "type": "zip" } }),
        json!({ "version": "1.0.0", "dist": 7 }),
        json!({ "version": "1.0.0", "source": { "type": "git", "url": "u" } }),
        json!({ "version": "1.0.0", "require": ["php"] }),
        json!({ "version": "1.0.0", "bin": "bin/widget" }),
        json!({ "version": 1 }),
    ];

    for entry in malformed {
        let kept = expand_versions(vec![entry.clone()], false);
        assert!(kept.is_empty(), "expected {entry} to be dropped");
    }
}

#[test]
fn a_field_the_type_does_not_know_is_ignored() {
    let entries = vec![json!({ "version": "1.0.0", "funding": [{ "type": "github" }] })];

    // A registry adds fields over time, so an unrecognized one is not a reason
    // to lose the version it sits on.
    assert_eq!(expanded(entries, false), ["1.0.0"]);
}

#[test]
fn an_unrecognized_dist_or_source_type_is_preserved() {
    let entries = vec![json!({
        "version": "1.0.0",
        "dist": { "type": "tar", "url": "https://example.com/1.0.0.tar" },
        "source": { "type": "hg", "url": "https://example.com/widget", "reference": "c1" },
    })];

    // Neither is installable, but rejecting them here would drop the version
    // outright; they are carried so the lock can round-trip what was published.
    let versions = expand_versions(entries, false);
    assert!(versions[0].dist.is_some());
    assert!(versions[0].source.is_some());
}
