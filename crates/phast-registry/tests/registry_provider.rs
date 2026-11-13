use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use phast_core::package::PackageName;
use phast_registry::filter::Rejection;
use phast_registry::http::cache::Cache;
use phast_registry::http::client::HttpClient;
use phast_registry::provider::{Lockable, RegistryProvider};
use phast_registry::router::RegistryOptions;
use phast_resolver::provider::Provider;
use phast_version::Stability;
use phast_version::parser;

use serde_json::{Value as JsonValue, json};
use tempfile::TempDir;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The metadata-url template a v2 registry publishes, relative to its base.
const METADATA_URL: &str = "/p2/%package%.json";

/// The package every test asks about.
fn name() -> PackageName {
    PackageName::new("vendor/pkg")
}

/// The filters these tests run under unless one varies them: a stable floor and
/// no age limit, so nothing is held back for a reason the test did not ask for.
fn options() -> RegistryOptions {
    RegistryOptions {
        minimum_stability: Stability::Stable,
        minimum_package_age: None,
        ..Default::default()
    }
}

/// A provider reading `server`, caching into `dir` and filtering by `options`.
fn provider(server: &MockServer, dir: &Path, options: RegistryOptions) -> RegistryProvider {
    RegistryProvider::new(server.uri(), HttpClient::new().unwrap(), options)
        .with_cache(Cache::new(dir))
}

/// Answer discovery on `server` with `template` as the metadata-url, which is
/// what marks the registry as speaking v2.
async fn discovery(server: &MockServer, template: &str) {
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "metadata-url": template,
        })))
        .mount(server)
        .await;
}

/// Serve `body` at `route`, the shape a p2 metadata file takes.
async fn serve(server: &MockServer, route: &str, body: JsonValue) {
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

/// Answer `route` with a 404, how a registry says it publishes no such resource.
async fn absent(server: &MockServer, route: &str) {
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(404))
        .mount(server)
        .await;
}

/// The p2 body listing `versions` under `vendor/pkg`.
fn published(versions: JsonValue) -> JsonValue {
    json!({ "packages": { "vendor/pkg": versions } })
}

/// The versions `provider` offers for `vendor/pkg`, newest first as it returns them.
async fn versions(provider: &RegistryProvider) -> Vec<String> {
    provider
        .versions(&name())
        .await
        .unwrap()
        .iter()
        .map(|version| version.to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Detecting which API the registry speaks
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_v2_registry_is_found_through_its_metadata_url() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;
    serve(
        &server,
        "/p2/vendor/pkg.json",
        published(json!([{ "version": "2.0.0" }, { "version": "1.0.0" }])),
    )
    .await;
    absent(&server, "/p2/vendor/pkg~dev.json").await;

    let tmp = TempDir::new().unwrap();
    assert_eq!(
        versions(&provider(&server, tmp.path(), options())).await,
        ["2.0", "1.0"],
    );
}

#[tokio::test]
async fn an_absolute_metadata_url_is_used_as_published() {
    let server = MockServer::start().await;
    let template = format!("{}/custom/%package%.json", server.uri());
    discovery(&server, &template).await;
    serve(
        &server,
        "/custom/vendor/pkg.json",
        published(json!([{ "version": "1.0.0" }])),
    )
    .await;
    absent(&server, "/custom/vendor/pkg~dev.json").await;

    // A registry may serve metadata from another host entirely, so a template
    // that already parses as a URL is not joined onto the base.
    let tmp = TempDir::new().unwrap();
    assert_eq!(
        versions(&provider(&server, tmp.path(), options())).await,
        ["1.0"],
    );
}

#[tokio::test]
async fn v2_discovery_happens_once_for_the_whole_resolve() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "metadata-url": METADATA_URL,
        })))
        .expect(1)
        .mount(&server)
        .await;

    for package in ["vendor/pkg-a", "vendor/pkg-b"] {
        serve(
            &server,
            &format!("/p2/{package}.json"),
            json!({ "packages": { package: [{ "version": "1.0.0" }] } }),
        )
        .await;
        absent(&server, &format!("/p2/{package}~dev.json")).await;
    }

    // A resolve touches hundreds of packages, so the API mode is settled once
    // and every later lookup goes straight to its metadata file.
    let tmp = TempDir::new().unwrap();
    let provider = provider(&server, tmp.path(), options());
    provider
        .versions(&PackageName::new("vendor/pkg-a"))
        .await
        .unwrap();
    provider
        .versions(&PackageName::new("vendor/pkg-b"))
        .await
        .unwrap();
}

#[tokio::test]
async fn a_v1_registry_serves_every_package_inline() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "packages": {
                "vendor/pkg": {
                    "1.0.0": { "version": "1.0.0" },
                    "2.0.0": { "version": "2.0.0" },
                },
            },
        })))
        // No metadata-url and the packages are the response, so one fetch holds
        // everything a Satis registry has to say.
        .expect(1)
        .mount(&server)
        .await;

    let tmp = TempDir::new().unwrap();
    assert_eq!(
        versions(&provider(&server, tmp.path(), options())).await,
        ["2.0", "1.0"],
    );
}

// ---------------------------------------------------------------------------
// Fetching a package's metadata
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_package_with_only_dev_branches_is_still_found() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;
    absent(&server, "/p2/vendor/pkg.json").await;
    serve(
        &server,
        "/p2/vendor/pkg~dev.json",
        published(json!([{ "version": "dev-main" }])),
    )
    .await;

    // The two files are fetched separately and either may be missing, so a
    // package that has never been tagged is absent only if both 404.
    let tmp = TempDir::new().unwrap();
    let dev = RegistryOptions {
        minimum_stability: Stability::Dev,
        ..options()
    };
    assert_eq!(
        versions(&provider(&server, tmp.path(), dev)).await,
        ["dev-main"],
    );
}

// ---------------------------------------------------------------------------
// A package the registry does not have
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_package_a_v1_registry_does_not_carry_has_no_versions() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "packages": {
                "vendor/other": {
                    "1.0.0": { "version": "1.0.0" },
                },
            },
        })))
        .mount(&server)
        .await;

    // Absence is an answer, not a failure: an error would stop a chained lookup before it
    // reached the registry that does carry the package.
    let tmp = TempDir::new().unwrap();
    let provider = provider(&server, tmp.path(), options());
    assert!(versions(&provider).await.is_empty());
}

#[tokio::test]
async fn a_package_missing_from_both_files_has_no_versions() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;
    absent(&server, "/p2/vendor/pkg.json").await;
    absent(&server, "/p2/vendor/pkg~dev.json").await;

    let tmp = TempDir::new().unwrap();
    let provider = provider(&server, tmp.path(), options());
    assert!(versions(&provider).await.is_empty());

    // Asked for anyway, the metadata lookup fails, and a chain holds several registries,
    // so the message has to say which one came back empty.
    let version = parser::parse_version("1.0.0").unwrap();
    let error = match provider.metadata(&name(), &version).await {
        Ok(_) => panic!("an absent package has no metadata"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("vendor/pkg"), "{error}");
    assert!(error.contains(&server.uri()), "{error}");
}

// ---------------------------------------------------------------------------
// Remembering what the registry answered
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_absent_package_is_not_asked_for_twice() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;
    absent(&server, "/p2/vendor/pkg.json").await;
    absent(&server, "/p2/vendor/pkg~dev.json").await;

    let tmp = TempDir::new().unwrap();
    let provider = provider(&server, tmp.path(), options());
    assert!(versions(&provider).await.is_empty());
    assert!(versions(&provider).await.is_empty());

    // A 404 is an answer, so it is remembered: the resolver asks about a package
    // once per candidate it considers, and a platform requirement absent from a
    // registry would otherwise be re-fetched all through the solve.
    assert_eq!(metadata_requests(&server).await, 2);
}

#[tokio::test]
async fn a_transient_failure_is_asked_again() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;
    Mock::given(method("GET"))
        .and(path("/p2/vendor/pkg.json"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let tmp = TempDir::new().unwrap();
    let provider = provider(&server, tmp.path(), options());
    let error = match provider.versions(&name()).await {
        Ok(_) => panic!("a 500 must not resolve"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("HTTP 500"), "{error}");

    // Unlike a 404 this says nothing about whether the package exists, so it is
    // not remembered and the next caller tries again.
    assert!(provider.versions(&name()).await.is_err());
    assert_eq!(metadata_requests(&server).await, 2);
}

#[tokio::test]
async fn tagged_and_dev_metadata_are_revalidated_separately() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;

    // Tagged resource: the first request (no conditional header) serves the body
    // with an ETag; a later request carrying that ETag is answered with 304.
    Mock::given(method("GET"))
        .and(path("/p2/vendor/pkg.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"tag-v1\"")
                .set_body_json(published(json!([{ "version": "1.0.0" }]))),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/p2/vendor/pkg.json"))
        .and(header("if-none-match", "\"tag-v1\""))
        .respond_with(ResponseTemplate::new(304))
        .expect(1)
        .mount(&server)
        .await;

    // Dev resource: same shape, a distinct ETag. If tagged and dev shared one
    // cache slot, this ETag would clobber the tagged one (or vice versa).
    Mock::given(method("GET"))
        .and(path("/p2/vendor/pkg~dev.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"dev-v1\"")
                .set_body_json(published(json!([{ "version": "dev-main" }]))),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/p2/vendor/pkg~dev.json"))
        .and(header("if-none-match", "\"dev-v1\""))
        .respond_with(ResponseTemplate::new(304))
        .expect(1)
        .mount(&server)
        .await;

    let tmp = TempDir::new().unwrap();

    // First resolve: both files fetched fresh (200) and cached in their own slots.
    let first = versions(&provider(&server, tmp.path(), options())).await;

    // Second resolve, fresh provider over the same cache dir: each file is
    // conditionally re-fetched with ITS OWN ETag and answered from disk (304).
    // Under a shared slot the wrong ETag would be sent, the 200 mock is spent,
    // no mock would match, and the tagged version would drop out.
    let second = versions(&provider(&server, tmp.path(), options())).await;

    // Each mount's `.expect(1)` (verified on server Drop) proves the tagged and
    // dev resources were each served once fresh and once from their own 304 slot.
    assert_eq!(
        first, second,
        "the cached (304) resolve must match the fresh one",
    );
}

/// How many requests `server` has taken for a metadata file, discovery aside.
async fn metadata_requests(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path().starts_with("/p2/"))
        .count()
}

// ---------------------------------------------------------------------------
// Filtering which versions are offered
// ---------------------------------------------------------------------------

#[tokio::test]
async fn versions_come_back_newest_first() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;
    serve(
        &server,
        "/p2/vendor/pkg.json",
        published(json!([
            { "version": "1.0.0" },
            { "version": "3.0.0" },
            { "version": "2.0.0" },
        ])),
    )
    .await;
    absent(&server, "/p2/vendor/pkg~dev.json").await;

    // The solver takes the first candidate that fits, so the order here is what
    // decides that the newest release wins rather than whichever was published.
    let tmp = TempDir::new().unwrap();
    assert_eq!(
        versions(&provider(&server, tmp.path(), options())).await,
        ["3.0", "2.0", "1.0"],
    );
}

#[tokio::test]
async fn a_version_below_the_stability_floor_is_dropped() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;
    serve(
        &server,
        "/p2/vendor/pkg.json",
        published(json!([{ "version": "dev-main" }])),
    )
    .await;
    absent(&server, "/p2/vendor/pkg~dev.json").await;

    let tmp = TempDir::new().unwrap();
    assert!(
        versions(&provider(&server, tmp.path(), options()))
            .await
            .is_empty(),
    );
}

#[tokio::test]
async fn a_stability_override_lowers_the_floor_for_one_package_only() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;
    serve(
        &server,
        "/p2/vendor/pkg.json",
        published(json!([{ "version": "dev-main" }])),
    )
    .await;
    absent(&server, "/p2/vendor/pkg~dev.json").await;

    // `^dev@dev` on one requirement must not lower the floor for the rest, so
    // the override is keyed and an entry for another package changes nothing.
    let tmp = TempDir::new().unwrap();
    let elsewhere = RegistryOptions {
        stability_overrides: HashMap::from([(PackageName::new("other/pkg"), Stability::Dev)]),
        ..options()
    };
    assert!(
        versions(&provider(&server, tmp.path(), elsewhere))
            .await
            .is_empty(),
    );

    let tmp = TempDir::new().unwrap();
    let here = RegistryOptions {
        stability_overrides: HashMap::from([(name(), Stability::Dev)]),
        ..options()
    };
    assert_eq!(
        versions(&provider(&server, tmp.path(), here)).await,
        ["dev-main"],
    );
}

#[tokio::test]
async fn a_release_published_inside_the_age_window_is_held_back() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;
    let hour_ago = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    let month_ago = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    serve(
        &server,
        "/p2/vendor/pkg.json",
        published(json!([
            { "version": "2.0.0", "time": hour_ago },
            { "version": "1.0.0", "time": month_ago },
        ])),
    )
    .await;
    absent(&server, "/p2/vendor/pkg~dev.json").await;

    // A day's delay is the window in which a compromised release is usually
    // pulled, so a version published inside it is not offered yet.
    let tmp = TempDir::new().unwrap();
    let delayed = RegistryOptions {
        minimum_package_age: Some(Duration::from_secs(24 * 60 * 60)),
        ..options()
    };
    assert_eq!(
        versions(&provider(&server, tmp.path(), delayed)).await,
        ["1.0"],
    );
}

#[tokio::test]
async fn a_release_with_no_usable_timestamp_clears_the_age_window() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;
    serve(
        &server,
        "/p2/vendor/pkg.json",
        published(json!([
            { "version": "2.0.0" },
            { "version": "1.0.0", "time": "the day before yesterday" },
        ])),
    )
    .await;
    absent(&server, "/p2/vendor/pkg~dev.json").await;

    // A registry that publishes no timestamp, or one nobody can read, would
    // otherwise have every one of its releases held back forever.
    let tmp = TempDir::new().unwrap();
    let delayed = RegistryOptions {
        minimum_package_age: Some(Duration::from_secs(24 * 60 * 60)),
        ..options()
    };
    assert_eq!(
        versions(&provider(&server, tmp.path(), delayed)).await,
        ["2.0", "1.0"],
    );
}

#[tokio::test]
async fn a_rejected_version_is_recorded_with_the_reason() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;
    let hour_ago = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    let month_ago = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    serve(
        &server,
        "/p2/vendor/pkg.json",
        published(json!([
            { "version": "2.0.0", "time": hour_ago },
            { "version": "1.1.0-beta1", "time": month_ago },
            { "version": "1.0.0", "time": month_ago },
        ])),
    )
    .await;
    absent(&server, "/p2/vendor/pkg~dev.json").await;

    let tmp = TempDir::new().unwrap();
    let delayed = RegistryOptions {
        minimum_package_age: Some(Duration::from_secs(24 * 60 * 60)),
        ..options()
    };
    let provider = provider(&server, tmp.path(), delayed);
    assert_eq!(versions(&provider).await, ["1.0"]);
    assert_eq!(versions(&provider).await, ["1.0"]);

    let rejections = provider.rejections();
    assert_eq!(rejections.len(), 1);

    let (rejected_name, rejected) = &rejections[0];
    assert_eq!(rejected_name, &name());
    assert_eq!(rejected.len(), 2, "recorded twice: {rejected:?}");

    let reason = |version: &str| {
        let version = parser::parse_version(version).unwrap();
        rejected
            .iter()
            .find(|(rejected, _)| *rejected == version)
            .map(|(_, rejection)| rejection.clone())
    };
    assert!(matches!(reason("2.0.0"), Some(Rejection::Age { .. })));
    assert_eq!(
        reason("1.1.0-beta1"),
        Some(Rejection::Stability {
            floor: Stability::Stable,
        }),
    );
}

// ---------------------------------------------------------------------------
// What a resolved version reports
// ---------------------------------------------------------------------------

#[tokio::test]
async fn replace_and_provide_specs_resolve_to_the_package_version() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;

    // laravel/framework@11.0.0 replaces illuminate/auth (self.version) and
    // spatie/once (*): both non-version sentinels that must not be dropped.
    serve(
        &server,
        "/p2/laravel/framework.json",
        json!({ "packages": { "laravel/framework": [{
            "version": "11.0.0",
            "replace": { "illuminate/auth": "self.version", "spatie/once": "*" },
            "provide": { "psr/container-implementation": "self.version" },
        }]}}),
    )
    .await;
    absent(&server, "/p2/laravel/framework~dev.json").await;

    let tmp = TempDir::new().unwrap();
    let provider = provider(&server, tmp.path(), options());
    let name = PackageName::new("laravel/framework");
    let version = parser::parse_version("11.0.0").unwrap();
    let meta = provider.metadata(&name, &version).await.unwrap();

    let replaced: std::collections::HashSet<_> = meta.replace.into_iter().collect();
    assert_eq!(
        replaced,
        std::collections::HashSet::from([
            (PackageName::new("illuminate/auth"), version.clone()),
            (PackageName::new("spatie/once"), version.clone()),
        ]),
    );
    assert_eq!(
        meta.provide,
        vec![(PackageName::new("psr/container-implementation"), version)],
    );
}

#[tokio::test]
async fn require_specs_resolve_to_the_package_version() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;

    // laravel/framework@11.0.0 requires illuminate/auth at self.version: the
    // sentinel binds to 11.0.0, exactly like the replace/provide ones.
    serve(
        &server,
        "/p2/laravel/framework.json",
        json!({ "packages": { "laravel/framework": [{
            "version": "11.0.0",
            "require": { "illuminate/auth": "self.version", "psr/log": "^1.0" },
        }]}}),
    )
    .await;
    absent(&server, "/p2/laravel/framework~dev.json").await;

    let tmp = TempDir::new().unwrap();
    let provider = provider(&server, tmp.path(), options());
    let name = PackageName::new("laravel/framework");
    let version = parser::parse_version("11.0.0").unwrap();
    let mut meta = provider.metadata(&name, &version).await.unwrap();

    // The raw map iterates unordered, so sort by name before comparing.
    meta.require.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    assert_eq!(
        meta.require,
        vec![
            (
                PackageName::new("illuminate/auth"),
                phast_version::Constraint::Exact(version.clone()),
            ),
            (
                PackageName::new("psr/log"),
                parser::parse_requirement("^1.0").unwrap().0
            ),
        ],
    );
}

#[tokio::test]
async fn metadata_for_a_version_the_registry_never_published_is_an_error() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;
    serve(
        &server,
        "/p2/vendor/pkg.json",
        published(json!([{ "version": "1.0.0" }])),
    )
    .await;
    absent(&server, "/p2/vendor/pkg~dev.json").await;

    let tmp = TempDir::new().unwrap();
    let provider = provider(&server, tmp.path(), options());
    let absent = parser::parse_version("9.9.9").unwrap();
    let error = match provider.metadata(&name(), &absent).await {
        Ok(_) => panic!("an unpublished version has no metadata"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("9.9.9"), "{error}");
    assert!(error.contains("vendor/pkg"), "{error}");
}

// ---------------------------------------------------------------------------
// The lock entry
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_lock_entry_carries_what_the_registry_published() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;
    serve(
        &server,
        "/p2/vendor/pkg.json",
        published(json!([{
            "version": "v1.0.0",
            "type": "library",
            "time": "2026-01-01T00:00:00+00:00",
            "license": ["MIT"],
            "dist": {
                "type": "zip",
                "url": "https://example.com/pkg-1.0.0.zip",
                "reference": "abc123",
                "shasum": "deadbeef",
            },
            "source": {
                "type": "git",
                "url": "https://github.com/vendor/pkg.git",
                "reference": "abc123",
            },
            "require": { "php": "^8.1" },
            "provide": { "psr/log-implementation": "1.0" },
            "extra": { "branch-alias": { "dev-main": "1.x-dev" } },
        }])),
    )
    .await;
    absent(&server, "/p2/vendor/pkg~dev.json").await;

    let tmp = TempDir::new().unwrap();
    let provider = provider(&server, tmp.path(), options());
    let version = parser::parse_version("1.0.0").unwrap();
    let locked = provider.lock(&name(), &version).await.unwrap();

    // The published string, not the parsed one: `%prettyVersion%` in a dist URL
    // resolves against it, and the normalized form would render `v1.0.0` as 1.0.
    assert_eq!(locked.version, "v1.0.0");
    assert_eq!(locked.package_type.as_deref(), Some("library"));
    assert_eq!(locked.time.as_deref(), Some("2026-01-01T00:00:00+00:00"));
    assert_eq!(locked.license, ["MIT"]);

    let dist = locked.dist.expect("a registry package is downloaded");
    assert_eq!(dist.url, "https://example.com/pkg-1.0.0.zip");
    assert_eq!(dist.shasum.as_deref(), Some("deadbeef"));

    let source = locked.source.expect("the git source is recorded too");
    assert_eq!(source.reference, "abc123");

    assert_eq!(
        locked.provide.entries.get("psr/log-implementation"),
        Some(&"1.0".to_owned()),
    );
    let extra = locked.extra.expect("extra is carried into the lock");
    assert_eq!(extra.data["branch-alias"]["dev-main"], "1.x-dev");
}

#[tokio::test]
async fn locking_a_version_that_was_never_resolved_is_an_error() {
    let server = MockServer::start().await;
    discovery(&server, METADATA_URL).await;
    serve(
        &server,
        "/p2/vendor/pkg.json",
        published(json!([{ "version": "1.0.0" }])),
    )
    .await;
    absent(&server, "/p2/vendor/pkg~dev.json").await;

    let tmp = TempDir::new().unwrap();
    let provider = provider(&server, tmp.path(), options());
    let never = parser::parse_version("9.9.9").unwrap();

    // Locking reads what resolution already fetched, so a version that was never
    // a candidate is a bug in the caller rather than something to go and fetch.
    let error = match provider.lock(&name(), &never).await {
        Ok(_) => panic!("an unresolved version has no lock entry"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("9.9.9"), "{error}");
}
