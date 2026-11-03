use std::str::FromStr;

use phast_core::package::PackageName;
use phast_registry::p2::parser::ParsedPackage;

use serde_json::{Value as JsonValue, json};

const NAME: &str = "league/commonmark";

/// A p2 response for `NAME` carrying `entries` as its version list, in the plain
/// format where each entry stands on its own.
fn response(entries: JsonValue) -> String {
    json!({ "packages": { NAME: entries } }).to_string()
}

/// The versions `json` parses to, rendered as the resolver names them.
fn versions(json: &str) -> Vec<String> {
    let parsed = ParsedPackage::from_p2_response(&PackageName::new(NAME), json).unwrap();
    parsed.versions.iter().map(|(v, _)| v.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Which versions a response yields
// ---------------------------------------------------------------------------

#[test]
fn every_entry_becomes_a_version() {
    let json = response(json!([
        { "version": "1.0.0", "require": {} },
        { "version": "2.0.0", "require": {} },
    ]));

    assert_eq!(versions(&json), ["1.0", "2.0"]);
}

#[test]
fn a_minified_response_is_merged_forward() {
    let json = json!({
        "minified": "composer/2.0",
        "packages": { NAME: [
            { "version": "2.0.0", "require": { "php": "^8.1" } },
            { "version": "1.0.0" },
        ]},
    })
    .to_string();

    // The flag on the response is what turns the entries into deltas, so the
    // older release picks up the requirement the newer one declared.
    let parsed = ParsedPackage::from_p2_response(&PackageName::new(NAME), &json).unwrap();
    let (_, older) = &parsed.versions[1];
    assert_eq!(older.require.get("php").map(String::as_str), Some("^8.1"));
}

#[test]
fn only_composer_2_0_marks_a_response_minified() {
    let json = json!({
        "minified": "composer/3.0",
        "packages": { NAME: [
            { "version": "2.0.0", "require": { "php": "^8.1" } },
            { "version": "1.0.0" },
        ]},
    })
    .to_string();

    // A format nobody has taught the parser to merge is read verbatim rather
    // than guessed at, so the older release inherits nothing.
    let parsed = ParsedPackage::from_p2_response(&PackageName::new(NAME), &json).unwrap();
    assert!(parsed.versions[1].1.require.is_empty());
}

#[test]
fn the_normalized_version_is_preferred_over_the_published_one() {
    let json = response(json!([
        { "version": "v2.8.0", "version-normalized": "2.8.0.0" },
    ]));

    // The published string carries whatever the tag was named; the normalized
    // one is already canonical, so it is the one parsed.
    assert_eq!(versions(&json), ["2.8"]);
}

// ---------------------------------------------------------------------------
// Offering a branch under its numeric alias
// ---------------------------------------------------------------------------

#[test]
fn a_branch_alias_is_offered_alongside_the_branch() {
    let json = response(json!([
        { "version": "2.8.0", "require": {} },
        {
            "version": "dev-main",
            "require": {},
            "extra": { "branch-alias": { "dev-main": "2.9-dev" } },
        },
    ]));

    // The alias comes first and the branch as published follows, so a numeric
    // require can select the branch while `dev-main` still names it.
    assert_eq!(versions(&json), ["2.8", "2.9.x-dev", "dev-main"]);

    let parsed = ParsedPackage::from_p2_response(&PackageName::new(NAME), &json).unwrap();
    let caret = phast_version::Constraint::from_str("^2.9").unwrap();
    assert!(
        parsed.versions.iter().any(|(v, _)| caret.matches(v)),
        "no version satisfied ^2.9",
    );
}

#[test]
fn an_alias_the_branch_cannot_use_leaves_only_the_branch() {
    let cases = [
        // The alias names a different branch.
        json!({ "branch-alias": { "dev-next": "2.9-dev" } }),
        json!({ "branch-alias": { "dev-main": 29 } }),
        json!({ "branch-alias": { "dev-main": "not a version" } }),
        json!({ "branch-alias": {} }),
        json!({}),
    ];

    for extra in cases {
        let json = response(json!([{ "version": "dev-main", "extra": extra }]));
        assert_eq!(versions(&json), ["dev-main"], "for extra {extra}");
    }
}

#[test]
fn a_branch_alias_on_a_numeric_version_is_ignored() {
    let json = response(json!([
        { "version": "2.8.0", "extra": { "branch-alias": { "dev-main": "2.9-dev" } } },
    ]));

    // The alias describes where the branch points, and a tagged release is not
    // a branch, so carrying the block does not earn it a second version.
    assert_eq!(versions(&json), ["2.8"]);
}

// ---------------------------------------------------------------------------
// A response the parser cannot use
// ---------------------------------------------------------------------------

#[test]
fn an_entry_without_a_usable_version_is_skipped() {
    let json = response(json!([
        { "version": "2.8.0" },
        { "version": "not-a-version!!!" },
        { "require": {} },
    ]));

    // One bad entry costs its own version and no more, so a registry with a
    // single malformed release still resolves.
    assert_eq!(versions(&json), ["2.8"]);
}

#[test]
fn a_package_with_no_entries_yields_no_versions() {
    assert!(versions(&response(json!([]))).is_empty());
}

#[test]
fn a_response_for_another_package_yields_no_versions() {
    let json = json!({ "packages": { "symfony/console": [{ "version": "7.0.0" }] } });

    // Entries are looked up by name rather than taken as the only key present,
    // so a registry answering with the wrong package reads as an empty one
    // instead of offering symfony/console's releases under this name.
    assert!(versions(&json.to_string()).is_empty());
}

#[test]
fn a_response_that_is_not_json_names_the_package_in_the_error() {
    let error = match ParsedPackage::from_p2_response(&PackageName::new(NAME), "not json") {
        Ok(_) => panic!("expected a parse error"),
        Err(error) => format!("{error:#}"),
    };

    // A registry serving an HTML error page is the common case, and the message
    // has to say which package was being fetched to be worth anything.
    assert!(error.contains(NAME), "{error}");
}

#[test]
fn a_response_with_no_packages_key_is_an_error() {
    // Distinct from a response that names another package: this one is not a p2
    // response at all, so it is reported rather than read as no versions.
    let parsed = ParsedPackage::from_p2_response(&PackageName::new(NAME), "{}");
    assert!(parsed.is_err());
}

// ---------------------------------------------------------------------------
// A v1 registry's inline package entry
// ---------------------------------------------------------------------------

#[test]
fn a_v1_entry_keyed_by_version_becomes_a_version() {
    let parsed = ParsedPackage::from_v1(&json!({
        "1.0.0": { "version": "1.0.0", "require": { "php": "^8.0" } },
        "2.0.0": { "version": "2.0.0" },
    }));

    assert_eq!(v1_versions(parsed), ["1.0", "2.0"]);
}

#[test]
fn a_v1_branch_alias_is_expanded_too() {
    let parsed = ParsedPackage::from_v1(&json!({
        "dev-main": {
            "version": "dev-main",
            "extra": { "branch-alias": { "dev-main": "2.9-dev" } },
        },
    }));

    // A Satis registry pins branches the same way Packagist does, so the alias
    // is read from the entry rather than from which format served it.
    assert_eq!(v1_versions(parsed), ["2.9.x-dev", "dev-main"]);
}

#[test]
fn a_v1_entry_that_does_not_deserialize_is_dropped() {
    let parsed = ParsedPackage::from_v1(&json!({
        "1.0.0": { "version": "1.0.0" },
        "2.0.0": { "version": "2.0.0", "dist": { "type": "zip" } },
        "3.0.0": { "require": {} },
    }));

    // A dist with no url and an entry naming no version both go; the releases
    // around them are still offered.
    assert_eq!(v1_versions(parsed), ["1.0"]);
}

#[test]
fn a_v1_value_that_is_not_an_object_is_not_a_package() {
    // The value sits under a package name in `packages.json` and a registry may
    // put anything there, so the shape is checked before it is read.
    assert!(ParsedPackage::from_v1(&json!([1, 2])).is_none());
    assert!(ParsedPackage::from_v1(&json!("2.0.0")).is_none());
    assert!(ParsedPackage::from_v1(&JsonValue::Null).is_none());
    assert!(ParsedPackage::from_v1(&json!({})).is_some());
}

/// The versions a `from_v1` result holds, sorted, since it is built from a JSON
/// object and the key order is the registry's rather than the version order.
fn v1_versions(parsed: Option<ParsedPackage>) -> Vec<String> {
    let parsed = parsed.expect("a JSON object is a package");
    let mut rendered: Vec<String> = parsed.versions.iter().map(|(v, _)| v.to_string()).collect();
    rendered.sort();
    rendered
}
