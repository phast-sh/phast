use phast_core::package::PackageName;
use phast_registry::http::client::HttpClient;
use phast_registry::provider::{InlineProvider, Lockable};
use phast_registry::vcs::host_for;
use phast_resolver::provider::Provider;

use serde_json::{Value as JsonValue, json};

const NAME: &str = "nst/json-test-suite";
const VERSION: &str = "2024.12.14";
const GIT_URL: &str = "https://github.com/nst/JSONTestSuite.git";
const REFERENCE: &str = "1ef36fa01286573e846ac449e8683f8833c5b26a";

/// A self-describing inline package (a JSON test-suite corpus), with every field
/// `build` reads declared. No manifest is ever fetched for it.
fn spec() -> JsonValue {
    json!({
        "name": NAME,
        "version": VERSION,
        "type": "library",
        "source": {
            "type": "git",
            "url": GIT_URL,
            "reference": REFERENCE
        },
        "require": {
            "php": ">=8.1",
            "acme/widget": "^1.2"
        },
        "extra": {
            "branch-alias": {
                "dev-main": "1.x-dev"
            }
        },
    })
}

/// The provider the router builds for `spec`, over the host its source url names.
fn provider(spec: &JsonValue) -> InlineProvider {
    let url = spec["source"]["url"].as_str().expect("spec declares a url");
    let host = host_for(url, HttpClient::new().unwrap()).expect("github is a supported host");
    InlineProvider::build(spec, host).unwrap()
}

/// The package name `spec` declares.
fn name() -> PackageName {
    PackageName::new(NAME)
}

// ---------------------------------------------------------------------------
// Reading the spec
// ---------------------------------------------------------------------------

#[test]
fn a_spec_missing_a_required_field_is_refused() {
    // Every message but the nameless one names the package, so a manifest with
    // several inline sources says which entry has to be fixed.
    let cases = [
        (json!({ "version": VERSION }), "declares no name"),
        (json!({ "name": NAME }), "declares no version"),
        // No source block at all is reported as the missing url, which is the
        // first field read out of it.
        (
            json!({ "name": NAME, "version": VERSION }),
            "declares no source url",
        ),
        (
            json!({ "name": NAME, "version": VERSION, "source": { "url": GIT_URL } }),
            "declares no source reference",
        ),
    ];

    for (spec, expected) in cases {
        let error = refusal(&spec);
        assert!(error.contains(expected), "for {spec}: {error}");
    }
}

#[test]
fn a_declared_version_that_does_not_parse_is_refused() {
    let spec = json!({
        "name": NAME,
        "version": "not a version",
        "source": { "type": "git", "url": GIT_URL, "reference": REFERENCE },
    });

    let error = refusal(&spec);
    assert!(error.contains("invalid version"), "{error}");
    assert!(error.contains(NAME), "{error}");
}

#[test]
fn a_require_constraint_that_is_not_a_string_is_refused() {
    let mut spec = spec();
    spec["require"] = json!({ "acme/widget": 1 });

    // A number where a constraint belongs is a manifest typo, not a package
    // with no requirement, so it cannot be skipped silently.
    let error = refusal(&spec);
    assert!(error.contains("acme/widget"), "{error}");
    assert!(error.contains("must be a string"), "{error}");
}

/// Why `build` refused `spec`, or a panic if it did not.
fn refusal(spec: &JsonValue) -> String {
    let host = host_for(GIT_URL, HttpClient::new().unwrap()).unwrap();
    match InlineProvider::build(spec, host) {
        Ok(_) => panic!("expected {spec} to be refused"),
        Err(error) => format!("{error:#}"),
    }
}

// ---------------------------------------------------------------------------
// Answering without the network
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_declared_version_is_the_only_one_offered() {
    let provider = provider(&spec());

    // The spec declares one version and there is no registry to ask for more,
    // so resolution has exactly this to choose from.
    let versions = provider.versions(&name()).await.unwrap();
    assert_eq!(
        versions.iter().map(|v| v.to_string()).collect::<Vec<_>>(),
        [VERSION],
    );
}

#[tokio::test]
async fn metadata_repeats_the_requires_the_spec_declared() {
    let provider = provider(&spec());
    let versions = provider.versions(&name()).await.unwrap();

    let metadata = provider.metadata(&name(), &versions[0]).await.unwrap();
    assert_eq!(
        requires(&metadata.require),
        ["acme/widget:^1.2", "php:>=8.1"],
    );

    // An inline spec has no way to declare these, so the resolver sees none
    // rather than whatever the repository's own composer.json happens to say.
    assert!(metadata.require_dev.is_empty());
    assert!(metadata.replace.is_empty());
    assert!(metadata.provide.is_empty());
}

#[tokio::test]
async fn versions_are_empty_for_a_name_the_provider_does_not_serve() {
    let provider = provider(&spec());

    // Not an error: an inline provider is pinned in the routing table under its
    // own name, so a miss here means the resolver asked the wrong provider and
    // an empty answer lets the real one supply the versions.
    let versions = provider.versions(&PackageName::new("someone/else")).await;
    assert!(versions.unwrap().is_empty());
}

#[tokio::test]
async fn locking_another_name_is_an_error() {
    let provider = provider(&spec());
    let other = PackageName::new("someone/else");
    let version = phast_version::parser::parse_version(VERSION).unwrap();

    // Locking happens after resolution has already picked this provider, so a
    // name it does not serve is a routing bug rather than a miss to fall through.
    let error = match provider.lock(&other, &version).await {
        Ok(_) => panic!("locking an unserved package must fail"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains(NAME), "got: {error}");
    assert!(error.contains("someone/else"), "got: {error}");
}

/// The resolved requirements rendered as `name:constraint`, so a failure reports
/// what was found rather than only that a match failed.
fn requires(require: &[(PackageName, phast_version::Constraint)]) -> Vec<String> {
    require
        .iter()
        .map(|(name, constraint)| format!("{}:{constraint}", name.as_str()))
        .collect()
}

// ---------------------------------------------------------------------------
// The lock entry
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_lock_entry_carries_a_dist_synthesized_from_the_git_source() {
    let locked = locked(&spec()).await;

    assert_eq!(locked.name, NAME);
    assert_eq!(locked.version, VERSION);

    // There is no registry to publish an archive, so the dist is built from the
    // pinned commit: whatever the branch moves to, this fetches the same bytes.
    let dist = locked.dist.expect("inline package has a dist");
    assert!(dist.url.ends_with(REFERENCE), "dist url: {}", dist.url);
    assert_eq!(dist.reference.as_deref(), Some(REFERENCE));

    let source = locked.source.expect("inline package has a source");
    assert_eq!(source.url, GIT_URL);
    assert_eq!(source.reference, REFERENCE);
}

#[tokio::test]
async fn the_requires_reach_the_lock_as_declared_constraints() {
    let locked = locked(&spec()).await;
    let entries: Vec<String> = locked
        .require
        .entries
        .iter()
        .map(|(name, constraint)| format!("{name}:{constraint}"))
        .collect();

    assert_eq!(entries, ["acme/widget:^1.2", "php:>=8.1"]);
}

#[tokio::test]
async fn type_and_extra_are_carried_into_the_lock() {
    let locked = locked(&spec()).await;

    // Both are inert to resolution and exist for what reads the lock afterwards:
    // the installer dispatches on type, and tooling reads extra.
    assert_eq!(locked.package_type.as_deref(), Some("library"));
    let extra = locked.extra.expect("inline package declares extra");
    assert_eq!(extra.data["branch-alias"]["dev-main"], "1.x-dev");
}

#[tokio::test]
async fn a_spec_declaring_no_type_or_extra_locks_without_them() {
    let mut spec = spec();
    spec["type"].take();
    spec["extra"].take();

    let locked = locked(&spec).await;
    assert!(locked.package_type.is_none());
    assert!(locked.extra.is_none());
}

#[tokio::test]
async fn an_extra_block_left_empty_by_stripping_nulls_is_dropped() {
    let mut spec = spec();
    spec["extra"] = json!({ "branch-alias": null });

    // TOML has no null, so the nulls go; an extra block that was nothing else is
    // then omitted rather than written as an empty table.
    let locked = locked(&spec).await;
    assert!(locked.extra.is_none());
}

/// The lock entry `spec` produces for the version it declares.
async fn locked(spec: &JsonValue) -> phast_core::lock::phast::PackageDef {
    let provider = provider(spec);
    let versions = provider.versions(&name()).await.unwrap();
    provider.lock(&name(), &versions[0]).await.unwrap()
}
