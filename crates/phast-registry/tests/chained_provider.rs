use std::collections::HashMap;
use std::path::Path;

use phast_core::manifest::{Manifest, Repository, Source};
use phast_core::package::PackageName;
use phast_registry::http::client::HttpClient;
use phast_registry::provider::ChainedProvider;
use phast_registry::router::RegistryOptions;
use phast_resolver::provider::Provider;
use phast_resolver::resolution::Resolution;
use phast_version::parser::parse_version;

use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client() -> HttpClient {
    HttpClient::new().unwrap()
}

/// A manifest declaring `repositories` as Composer repos, pinning `sources`,
/// and reaching Packagist at `packagist`.
///
/// `packagist: None` switches the fallback off rather than leaving it pointed at
/// the real packagist.org, so a test that falls through the whole chain fails
/// instead of quietly reaching the network.
fn manifest(
    repositories: &[(&str, Option<&str>)],
    sources: HashMap<PackageName, Source>,
    packagist: Option<&str>,
) -> Manifest {
    let mut repositories: Vec<Repository> = repositories
        .iter()
        .map(|(url, name)| Repository::Composer {
            url: (*url).to_owned(),
            name: name.map(str::to_owned),
            default: false,
        })
        .collect();

    repositories.push(Repository::Packagist {
        url: packagist.map(str::to_owned),
        name: None,
        default: false,
    });

    Manifest {
        repositories,
        sources,
        ..Default::default()
    }
}

/// A chain over `manifest`, with the temp dir holding its cache.
///
/// The cache is its own directory rather than the real one, so a run neither
/// reads what an earlier run left behind nor writes into the user's cache.
async fn chain(manifest: &Manifest) -> (ChainedProvider, TempDir) {
    let cache = TempDir::new().unwrap();
    let chain = ChainedProvider::new(client(), cache.path().to_path_buf())
        .with_manifest(Path::new(""), manifest, RegistryOptions::default(), |_| {})
        .await
        .unwrap();

    (chain, cache)
}

/// The `packages.json` a v2 registry serves, pointing lookups at `server_uri`.
fn v2_packages_json(server_uri: &str) -> serde_json::Value {
    serde_json::json!({ "metadata-url": format!("{}/p2/%package%.json", server_uri) })
}

// ---------------------------------------------------------------------------
// Falling through a registry that does not have the package
// ---------------------------------------------------------------------------

#[tokio::test]
#[rustfmt::skip]
async fn falls_through_to_second_when_first_returns_not_found() {
    // First registry: v2, but the requested package returns 404.
    let registry1 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(v2_packages_json(&registry1.uri())))
        .mount(&registry1)
        .await;
    Mock::given(method("GET"))
        .and(path("/p2/vendor/pkg.json"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&registry1)
        .await;

    // Second registry: v2, the package exists.
    let registry2 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(v2_packages_json(&registry2.uri())))
        .mount(&registry2)
        .await;
    Mock::given(method("GET"))
        .and(path("/p2/vendor/pkg.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "packages": {
                "vendor/pkg": [{"version": "1.0.0"}]
            }
        })))
        .mount(&registry2)
        .await;

    let manifest = manifest(
        &[(&registry1.uri(), None), (&registry2.uri(), None)],
        HashMap::new(),
        None,
    );

    let (chain, _cache) = chain(&manifest).await;

    let versions = chain
        .versions(&PackageName::new("vendor/pkg"))
        .await
        .unwrap();
    assert_eq!(versions.len(), 1);
}

#[tokio::test]
#[rustfmt::skip]
async fn returns_empty_when_no_registry_has_the_package() {
    let registry1 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(v2_packages_json(&registry1.uri())))
        .mount(&registry1)
        .await;
    Mock::given(method("GET"))
        .and(path("/p2/vendor/pkg.json"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&registry1)
        .await;

    let registry2 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(v2_packages_json(&registry2.uri())))
        .mount(&registry2)
        .await;
    Mock::given(method("GET"))
        .and(path("/p2/vendor/pkg.json"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&registry2)
        .await;

    let manifest = manifest(
        &[(&registry1.uri(), None), (&registry2.uri(), None)],
        HashMap::new(),
        None,
    );

    let (chain, _cache) = chain(&manifest).await;

    let versions = chain
        .versions(&PackageName::new("vendor/pkg"))
        .await
        .unwrap();
    assert!(versions.is_empty());
}

// ---------------------------------------------------------------------------
// Stopping at the first registry that answers
// ---------------------------------------------------------------------------

#[tokio::test]
#[rustfmt::skip]
async fn first_provider_wins_when_package_is_found() {
    let registry1 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(v2_packages_json(&registry1.uri())))
        .mount(&registry1)
        .await;
    Mock::given(method("GET"))
        .and(path("/p2/vendor/pkg.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "packages": {
                "vendor/pkg": [
                    {"version": "3.0.0"},
                    {"version": "2.0.0"}
                ]
            }
        })))
        .mount(&registry1)
        .await;

    // Second registry also has the package, but with a different set of versions.
    let registry2 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(v2_packages_json(&registry2.uri())))
        .expect(0) // must not be reached
        .mount(&registry2)
        .await;

    let manifest = manifest(
        &[(&registry1.uri(), None), (&registry2.uri(), None)],
        HashMap::new(),
        None,
    );

    let (chain, _cache) = chain(&manifest).await;

    let versions = chain
        .versions(&PackageName::new("vendor/pkg"))
        .await
        .unwrap();
    assert_eq!(versions.len(), 2);
}

// ---------------------------------------------------------------------------
// Stopping at a registry that fails
// ---------------------------------------------------------------------------

#[tokio::test]
#[rustfmt::skip]
async fn stops_at_an_unavailable_registry_rather_than_trying_the_next() {
    // First registry: packages.json returns 500 (discovery failure).
    let registry1 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&registry1)
        .await;

    // Second registry: healthy v2, the package exists.
    let registry2 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(v2_packages_json(&registry2.uri())))
        .mount(&registry2)
        .await;
    Mock::given(method("GET"))
        .and(path("/p2/vendor/pkg.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "packages": {
                "vendor/pkg": [{"version": "2.0.0"}]
            }
        })))
        .mount(&registry2)
        .await;

    let manifest = manifest(
        &[(&registry1.uri(), None), (&registry2.uri(), None)],
        HashMap::new(),
        None,
    );

    let (chain, _cache) = chain(&manifest).await;

    // The second registry would answer, and that is the point: a failing registry stops
    // the lookup, or an outage of the first would hand its packages to whichever registry
    // comes next.
    let error = chain
        .versions(&PackageName::new("vendor/pkg"))
        .await
        .expect_err("a failing registry was passed over")
        .to_string();
    assert!(error.contains(&registry1.uri()), "the failing registry is not named: {error}");
}

// ---------------------------------------------------------------------------
// Routing packages that [sources] pins
// ---------------------------------------------------------------------------

#[tokio::test]
#[rustfmt::skip]
async fn sourced_package_routes_only_to_pinned_registry() {
    let nova = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(v2_packages_json(&nova.uri())))
        .mount(&nova)
        .await;
    Mock::given(method("GET"))
        .and(path("/p2/laravel/nova.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "packages": { "laravel/nova": [{ "version": "4.0.0" }] }
        })))
        .mount(&nova)
        .await;

    // satis is in the fallback chain but must not be consulted for a pinned package
    let satis = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(v2_packages_json(&satis.uri())))
        .expect(0)
        .mount(&satis)
        .await;

    let mut sources = HashMap::new();
    sources.insert(
        PackageName::new("laravel/nova"),
        Source::Repository { repository: "nova".to_owned() },
    );

    let manifest = manifest(
        &[(&nova.uri(), Some("nova")), (&satis.uri(), Some("satis"))],
        sources,
        None,
    );

    let (chain, _cache) = chain(&manifest).await;

    let versions = chain
        .versions(&PackageName::new("laravel/nova"))
        .await
        .unwrap();
    assert_eq!(versions.len(), 1);
}

#[tokio::test]
#[rustfmt::skip]
async fn sourced_registry_excluded_from_fallback() {
    // nova is pinned for laravel/nova; it must not appear in the fallback chain
    let nova = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(v2_packages_json(&nova.uri())))
        .expect(0)
        .mount(&nova)
        .await;

    let packagist = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(v2_packages_json(&packagist.uri())))
        .mount(&packagist)
        .await;
    Mock::given(method("GET"))
        .and(path("/p2/symfony/console.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "packages": { "symfony/console": [{ "version": "6.0.0" }] }
        })))
        .mount(&packagist)
        .await;

    let mut sources = HashMap::new();
    sources.insert(
        PackageName::new("laravel/nova"),
        Source::Repository { repository: "nova".to_owned() },
    );

    let manifest = manifest(
        &[(&nova.uri(), Some("nova"))],
        sources,
        Some(&packagist.uri()),
    );

    let (chain, _cache) = chain(&manifest).await;
    let versions = chain
        .versions(&PackageName::new("symfony/console"))
        .await
        .unwrap();
    assert_eq!(versions.len(), 1);
}

#[tokio::test]
#[rustfmt::skip]
async fn unknown_source_repository_falls_through_to_fallback() {
    let packagist = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(v2_packages_json(&packagist.uri())))
        .mount(&packagist)
        .await;
    Mock::given(method("GET"))
        .and(path("/p2/vendor/pkg.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "packages": { "vendor/pkg": [{ "version": "1.0.0" }] }
        })))
        .mount(&packagist)
        .await;

    let mut sources = HashMap::new();
    sources.insert(
        PackageName::new("vendor/pkg"),
        Source::Repository { repository: "nonexistent".to_owned() },
    );

    let manifest = manifest(&[], sources, Some(&packagist.uri()));

    let (chain, _cache) = chain(&manifest).await;
    let versions = chain
        .versions(&PackageName::new("vendor/pkg"))
        .await
        .unwrap();
    assert_eq!(versions.len(), 1);
}

// ---------------------------------------------------------------------------
// Reporting why a package could not be locked
// ---------------------------------------------------------------------------

#[tokio::test]
#[rustfmt::skip]
async fn enrich_reports_why_a_package_could_not_be_locked() {
    let registry = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(v2_packages_json(&registry.uri())))
        .mount(&registry)
        .await;
    // The registry serves 1.0.0 only.
    Mock::given(method("GET"))
        .and(path("/p2/vendor/pkg.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "packages": { "vendor/pkg": [{"version": "1.0.0"}] }
        })))
        .mount(&registry)
        .await;

    let manifest = manifest(&[(&registry.uri(), None)], HashMap::new(), None);
    let (chain, _cache) = chain(&manifest).await;

    // Ask to lock a version the registry never served. The provider fails, and the
    // reason must survive: `enrich` used to discard every error and report a generic
    // "no registry has cached metadata", which hid what actually went wrong.
    let name = PackageName::new("vendor/pkg");
    let resolution = Resolution {
        packages: HashMap::from([(name.clone(), parse_version("2.0.0").unwrap())]),
        dev_package_names: Default::default(),
    };

    let error = match chain
        .enrich(&resolution, "sha256:test", Default::default(), Default::default(), |_, _| {})
        .await
    {
        Ok(_) => panic!("locking an unserved version must fail"),
        Err(error) => format!("{error:#}"),
    };

    // `2.0`, not `2.0.0`: version rendering drops trailing zero segments.
    assert!(error.contains("could not lock vendor/pkg@2.0"), "got: {error}");
    assert!(error.contains("missing from cache"), "underlying cause lost: {error}");
}
