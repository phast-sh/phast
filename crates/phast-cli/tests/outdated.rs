use std::str::FromStr;

use phast_cli::commands::outdated::checker::{OutdatedChecker, UpdateKind, determine_kind};
use phast_core::manifest::Manifest;
use phast_registry::http::client::HttpClient;
use phast_version::Constraint;
use phast_version::parser::parse_version;

use indoc::indoc;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The project under test: one registry, one requirement at `^1.0`.
///
/// `{registry}` is replaced with the mock server's address, which is only known
/// once it has started.
const MANIFEST: &str = indoc! {r#"
    [package]
    name = "test/project"

    [[repositories]]
    type = "composer"
    url  = "{registry}"

    [require]
    "vendor/alpha" = "^1.0"
"#};

/// A checker over `server`, with the temp dirs it reads from.
///
/// Both dirs are returned because dropping either deletes it out from under the
/// checker; a caller binds them for the length of the test.
async fn checker(server: &MockServer) -> (OutdatedChecker, tempfile::TempDir, tempfile::TempDir) {
    let manifest_dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        manifest_dir.path().join("phast.toml"),
        MANIFEST.replace("{registry}", &server.uri()),
    )
    .unwrap();
    let manifest = Manifest::load(manifest_dir.path()).unwrap();

    let cache_dir = tempfile::TempDir::new().unwrap();
    let checker = OutdatedChecker::new(
        manifest_dir.path(),
        &manifest,
        HttpClient::new().unwrap(),
        cache_dir.path().to_path_buf(),
        |_| {},
    )
    .await
    .unwrap();

    (checker, manifest_dir, cache_dir)
}

/// A parsed version, for a string a test knows is valid.
fn version(string: &str) -> phast_version::Version {
    parse_version(string).expect("valid version string")
}

/// A parsed constraint, for a string a test knows is valid.
fn constraint(string: &str) -> Constraint {
    Constraint::from_str(string).expect("valid constraint string")
}

/// Mount the registry index, pointing metadata lookups at the p2 endpoint.
///
/// A package is only found if [`mount_package`] mounted it too; without that the
/// p2 request 404s, which is how a package with no known versions is arranged.
async fn mount_registry(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/packages.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "metadata-url": "/p2/%package%.json"
        })))
        .mount(server)
        .await;
}

/// Mount `name`'s p2 metadata listing `versions`, in the order given.
async fn mount_package(server: &MockServer, name: &str, versions: &[&str]) {
    let versions: Vec<_> = versions
        .iter()
        .map(|v| json!({ "version": v, "version-normalized": format!("{v}.0"), "require": {} }))
        .collect();

    Mock::given(method("GET"))
        .and(path(format!("/p2/{name}.json")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "packages": { name: versions } })),
        )
        .mount(server)
        .await;
}

// ---------------------------------------------------------------------------
// Classifying an update against the locked version
// ---------------------------------------------------------------------------

#[test]
fn patch_when_same_major_and_minor() {
    assert!(matches!(
        determine_kind(&version("1.2.3"), &version("1.2.4"), None),
        UpdateKind::Patch,
    ));
}

#[test]
fn minor_when_same_major_higher_minor() {
    assert!(matches!(
        determine_kind(&version("1.2.3"), &version("1.3.0"), None),
        UpdateKind::Minor,
    ));
}

#[test]
fn major_when_major_segment_increases() {
    assert!(matches!(
        determine_kind(&version("1.2.3"), &version("2.0.0"), None),
        UpdateKind::Major,
    ));
}

#[test]
fn major_when_latest_outside_constraint() {
    let constraint = constraint("^1.0");
    assert!(matches!(
        determine_kind(&version("1.9.0"), &version("2.0.0"), Some(&constraint)),
        UpdateKind::Major,
    ));
}

#[test]
fn minor_when_latest_within_constraint() {
    let constraint = constraint("^1.0");
    assert!(matches!(
        determine_kind(&version("1.2.0"), &version("1.5.0"), Some(&constraint)),
        UpdateKind::Minor,
    ));
}

#[test]
fn patch_when_latest_within_constraint() {
    let constraint = constraint("^1.2");
    assert!(matches!(
        determine_kind(&version("1.2.3"), &version("1.2.9"), Some(&constraint)),
        UpdateKind::Patch,
    ));
}

#[test]
fn major_when_locked_or_latest_is_branch() {
    let branch = phast_version::Version::Branch("main".to_string());
    assert!(matches!(
        determine_kind(&branch, &version("2.0.0"), None),
        UpdateKind::Major,
    ));
    assert!(matches!(
        determine_kind(&version("1.0.0"), &branch, None),
        UpdateKind::Major,
    ));
}

#[test]
fn patch_even_when_the_latest_patch_is_lower() {
    // The patch segment is never compared, so anything sharing a major and minor
    // lands here. A registry listing a lower patch as latest is not a new case.
    assert!(matches!(
        determine_kind(&version("1.2.9"), &version("1.2.1"), None),
        UpdateKind::Patch,
    ));
}

// ---------------------------------------------------------------------------
// Checking locked packages against the registry
// ---------------------------------------------------------------------------

#[tokio::test]
async fn checker_reports_minor_update() {
    let server = MockServer::start().await;
    mount_registry(&server).await;
    mount_package(&server, "vendor/alpha", &["1.1.1", "1.0.0"]).await;
    let (checker, _manifest_dir, _cache_dir) = checker(&server).await;

    let packages = vec![("vendor/alpha".to_string(), "1.0.0".to_string())];
    let result = checker.check(&packages).await.unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].package, "vendor/alpha");
    assert_eq!(result[0].locked, "1.0.0");
    assert_eq!(result[0].latest, "1.1.1");
    assert!(matches!(result[0].kind, UpdateKind::Minor));
}

#[tokio::test]
async fn checker_returns_empty_when_up_to_date() {
    let server = MockServer::start().await;
    mount_registry(&server).await;
    mount_package(&server, "vendor/alpha", &["1.0.0"]).await;
    let (checker, _manifest_dir, _cache_dir) = checker(&server).await;

    let packages = vec![("vendor/alpha".to_string(), "1.0.0".to_string())];
    let result = checker.check(&packages).await.unwrap();

    assert!(result.is_empty());
}

#[tokio::test]
async fn checker_classifies_major_when_outside_constraint() {
    let server = MockServer::start().await;
    mount_registry(&server).await;
    mount_package(&server, "vendor/alpha", &["2.0.0", "1.9.0"]).await;
    let (checker, _manifest_dir, _cache_dir) = checker(&server).await;

    // MANIFEST constrains vendor/alpha to ^1.0; latest 2.0.0 is outside it.
    let packages = vec![("vendor/alpha".to_string(), "1.9.0".to_string())];
    let result = checker.check(&packages).await.unwrap();

    assert_eq!(result.len(), 1);
    assert!(matches!(result[0].kind, UpdateKind::Major));
}

#[tokio::test]
async fn checker_takes_the_highest_version_the_registry_lists() {
    let server = MockServer::start().await;
    mount_registry(&server).await;
    mount_package(&server, "vendor/alpha", &["1.0.0", "1.1.1"]).await;
    let (checker, _manifest_dir, _cache_dir) = checker(&server).await;

    // Listed ascending here, where the other tests list descending. The provider
    // sorts, so the order a registry happens to answer in decides nothing.
    let packages = vec![("vendor/alpha".to_string(), "1.0.0".to_string())];
    let result = checker.check(&packages).await.unwrap();

    assert_eq!(result[0].latest, "1.1.1");
}

#[tokio::test]
async fn checker_keeps_the_order_the_packages_were_given_in() {
    let server = MockServer::start().await;
    mount_registry(&server).await;
    mount_package(&server, "vendor/alpha", &["1.1.1"]).await;
    mount_package(&server, "vendor/beta", &["2.0.0"]).await;
    let (checker, _manifest_dir, _cache_dir) = checker(&server).await;

    // The lookups run concurrently, so output order could follow whichever
    // request finished first. It follows the input instead.
    let packages = vec![
        ("vendor/beta".to_string(), "1.0.0".to_string()),
        ("vendor/alpha".to_string(), "1.0.0".to_string()),
    ];
    let result = checker.check(&packages).await.unwrap();

    let names: Vec<&str> = result.iter().map(|e| e.package.as_str()).collect();
    assert_eq!(names, vec!["vendor/beta", "vendor/alpha"]);
}

#[tokio::test]
async fn checker_skips_a_locked_version_it_cannot_parse() {
    let server = MockServer::start().await;
    mount_registry(&server).await;
    mount_package(&server, "vendor/alpha", &["1.1.1"]).await;
    let (checker, _manifest_dir, _cache_dir) = checker(&server).await;

    // A lock written by hand, or by something else, can hold anything. Reporting
    // it as outdated would compare a version that was never understood.
    let packages = vec![("vendor/alpha".to_string(), "not-a-version".to_string())];
    let result = checker.check(&packages).await.unwrap();

    assert!(result.is_empty());
}

#[tokio::test]
async fn checker_skips_package_when_registry_returns_no_versions() {
    let server = MockServer::start().await;
    // The index is mounted but the package is not, so its p2 request 404s.
    mount_registry(&server).await;
    let (checker, _manifest_dir, _cache_dir) = checker(&server).await;

    let packages = vec![("vendor/alpha".to_string(), "1.0.0".to_string())];
    let result = checker.check(&packages).await.unwrap();

    assert!(result.is_empty());
}
