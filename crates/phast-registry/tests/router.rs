use std::collections::HashMap;
use std::path::Path;
use std::ptr::eq;

use phast_core::manifest::{GitRef, Manifest, Source};
use phast_core::package::PackageName;
use phast_registry::http::cache::Cache;
use phast_registry::http::client::HttpClient;
use phast_registry::router::{PackageProvider, RegistryOptions, RegistryRouter};
use phast_resolver::provider::Provider;
use phast_version::Stability;

use indoc::indoc;
use tempfile::TempDir;

/// A host with no driver, so every source pointing at it is skipped.
const UNSUPPORTED: &str = "https://git.example.com/acme/widget.git";

/// The filters every router here is built with: a stable floor and no age limit.
fn options() -> RegistryOptions {
    RegistryOptions {
        minimum_stability: Stability::Stable,
        minimum_package_age: None,
        ..Default::default()
    }
}

/// A `[[repositories]]` entry for the registry at `url`, answerable by `name`
/// from `[sources]` when it has one.
fn registry<'a>(url: &'a str, name: Option<&'a str>) -> PackageProvider<'a> {
    PackageProvider::Registry { url, name }
}

/// A `[[repositories]]` entry for the package in the directory `rel_path`.
fn path(rel_path: &str) -> PackageProvider<'_> {
    PackageProvider::Path { rel_path }
}

/// A `[sources]` value pinning a package to the named `[[repositories]]` endpoint.
fn repository(name: &str) -> Source {
    Source::Repository {
        repository: name.to_owned(),
    }
}

/// A one-entry `[sources]` map, which is all any of these tests needs.
fn sources(name: &str, source: Source) -> HashMap<PackageName, Source> {
    HashMap::from([(PackageName::new(name), source)])
}

/// Write a package manifest for `name` at `rel` under `base`, declaring `version`.
fn package(base: &Path, rel: &str, name: &str, version: &str) {
    let dir = base.join(rel);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("phast.toml"),
        format!("[package]\nname    = \"{name}\"\nversion = \"{version}\"\n"),
    )
    .unwrap();
}

/// The router `repositories` and `sources` resolve to under `base`, paired with
/// every warning the resolve emitted.
async fn router(
    base: &Path,
    repositories: Vec<PackageProvider<'_>>,
    sources: &HashMap<PackageName, Source>,
) -> (RegistryRouter, Vec<String>) {
    let mut warnings = Vec::new();
    let mut router = RegistryRouter::new(HttpClient::new().unwrap(), Cache::default_root());
    router
        .resolve(base, repositories, sources, options(), |warning| {
            warnings.push(warning.to_owned())
        })
        .await
        .unwrap();
    (router, warnings)
}

/// A router over registries alone, where the project root cannot matter.
async fn registries(
    repositories: Vec<PackageProvider<'_>>,
    sources: &HashMap<PackageName, Source>,
) -> RegistryRouter {
    router(Path::new(""), repositories, sources).await.0
}

// ---------------------------------------------------------------------------
// Pinning a package to one repository
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_source_pins_a_package_to_the_repository_it_names() {
    let router = registries(
        vec![
            registry("https://nova.laravel.com", Some("nova")),
            registry("https://repo.packagist.org", None),
        ],
        &sources("laravel/nova", repository("nova")),
    )
    .await;

    // Index 0 is the nova endpoint, the first one declared.
    let nova = PackageName::new("laravel/nova");
    assert_eq!(router.sources.get(&nova), Some(&0));
    assert_eq!(router.route(&nova).len(), 1);
}

#[tokio::test]
async fn a_repository_with_no_name_cannot_be_named_by_a_source() {
    // Packagist is declared but anonymously, so there is nothing for the source
    // to reference and the pin has no target.
    let router = registries(
        vec![registry("https://repo.packagist.org", None)],
        &sources("vendor/pkg", repository("packagist")),
    )
    .await;

    assert!(router.sources.is_empty());
}

#[tokio::test]
async fn a_source_naming_a_repository_that_was_never_declared_is_ignored() {
    let router = registries(
        vec![registry("https://repo.packagist.org", None)],
        &sources("vendor/pkg", repository("nonexistent")),
    )
    .await;

    // The package falls through to the ordinary chain rather than failing the
    // resolve, so a stale pin left in a manifest does not stop an install.
    assert!(router.sources.is_empty());
}

#[tokio::test]
async fn a_path_repository_pins_itself_by_the_name_it_declares() {
    let tmp = TempDir::new().unwrap();
    package(tmp.path(), "packages/local", "acme/local", "1.0.0");

    let (router, _) = router(
        tmp.path(),
        vec![
            path("packages/local"),
            registry("https://repo.packagist.org", None),
        ],
        &HashMap::new(),
    )
    .await;

    // Nothing in [sources] names it: a path repository reads its own manifest and
    // pins itself, so a local checkout shadows the registry copy automatically.
    let local = PackageName::new("acme/local");
    assert_eq!(router.sources.get(&local), Some(&0));
    assert_eq!(router.route(&local).len(), 1);

    // An unrelated package still falls through to Packagist.
    assert_eq!(router.route(&PackageName::new("symfony/console")).len(), 1);
}

#[tokio::test]
async fn an_inline_source_pins_itself_by_the_name_it_declares() {
    let inline = Source::Package {
        package: serde_json::json!({
            "name": "nst/json-test-suite",
            "version": "2024.12.14",
            "source": {
                "type": "git",
                "url": "https://github.com/nst/JSONTestSuite.git",
                "reference": "1ef36fa01286573e846ac449e8683f8833c5b26a",
            },
        }),
    };
    let tmp = TempDir::new().unwrap();

    let (router, warnings) = router(
        tmp.path(),
        vec![registry("https://repo.packagist.org", None)],
        &sources("nst/json-test-suite", inline),
    )
    .await;

    // A package described entirely in the manifest still gets a provider of its
    // own, so it is never looked for on a registry that does not carry it.
    let corpus = PackageName::new("nst/json-test-suite");
    assert_eq!(router.route(&corpus).len(), 1);
    assert_eq!(router.fallback.len(), 1);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[tokio::test]
async fn a_path_source_supplies_the_version_the_package_declares_none_of() {
    // A monorepo component with no `version` in its manifest, pinned via a
    // `[sources]` path spec that declares the version (the `symfony/contracts`
    // case). The source `version` must supply the package's version.
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("src/Contracts")).unwrap();
    std::fs::write(
        tmp.path().join("src/Contracts/phast.toml"),
        indoc! {r#"
            [package]
            name = "acme/contracts"
        "#},
    )
    .unwrap();

    let source = Source::Path {
        path: "src/Contracts".to_owned(),
        version: Some("3.7.x-dev".to_owned()),
    };
    let (router, _) = router(
        tmp.path(),
        vec![registry("https://repo.packagist.org", None)],
        &sources("acme/contracts", source),
    )
    .await;

    let contracts = PackageName::new("acme/contracts");
    assert_eq!(router.route(&contracts).len(), 1);

    // `3.7.x-dev` is a branch-alias: a top-of-series dev version (so it satisfies a
    // numeric `^3.7`), rendered back with the `x` placeholder.
    let versions = router.route(&contracts)[0]
        .versions(&contracts)
        .await
        .unwrap();
    assert_eq!(
        versions.iter().map(|v| v.to_string()).collect::<Vec<_>>(),
        ["3.7.x-dev"],
    );
}

// ---------------------------------------------------------------------------
// Which repositories answer everything else
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_pinned_repository_is_left_out_of_the_fallback() {
    let router = registries(
        vec![
            registry("https://nova.laravel.com", Some("nova")),
            registry("https://repo.packagist.org", None),
        ],
        &sources("laravel/nova", repository("nova")),
    )
    .await;

    // A private endpoint reached only by pin is not asked about every other
    // package in the graph, which would leak the dependency list to it.
    assert_eq!(router.providers.len(), 2);
    assert_eq!(router.fallback.len(), 1);
}

#[tokio::test]
async fn every_repository_is_in_the_fallback_when_nothing_is_pinned() {
    let router = registries(
        vec![
            registry("https://nova.laravel.com", Some("nova")),
            registry("https://repo.packagist.org", None),
        ],
        &HashMap::new(),
    )
    .await;

    assert_eq!(router.providers.len(), router.fallback.len());
}

#[tokio::test]
async fn an_unpinned_package_is_routed_to_the_whole_fallback() {
    let router = registries(
        vec![
            registry("https://nova.laravel.com", Some("nova")),
            registry("https://repo.packagist.org", None),
        ],
        &sources("laravel/nova", repository("nova")),
    )
    .await;

    assert_eq!(
        router.route(&PackageName::new("symfony/console")).len(),
        router.fallback.len(),
    );
}

#[tokio::test]
async fn a_project_with_no_repositories_routes_nowhere() {
    let router = registries(vec![], &HashMap::new()).await;

    // An empty slice rather than an error: a manifest may disable Packagist and
    // declare nothing, and it is resolution that reports what it cannot find.
    let unpinned = PackageName::new("symfony/console");
    assert!(router.providers.is_empty());
    assert!(router.route(&unpinned).is_empty());
}

// ---------------------------------------------------------------------------
// Routing a package to the repository it was found in
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_found_package_is_routed_only_to_that_repository() {
    let router = registries(
        vec![
            registry("https://nova.laravel.com", None),
            registry("https://repo.packagist.org", None),
        ],
        &HashMap::new(),
    )
    .await;

    let found = PackageName::new("laravel/nova");
    router.found(&found, 1);

    let route = router.route(&found);
    assert_eq!(route.len(), 1);
    assert!(eq(&route[0], &router.fallback[1]));

    let elsewhere = PackageName::new("symfony/console");
    assert_eq!(router.route(&elsewhere).len(), router.fallback.len());
}

#[tokio::test]
async fn a_found_package_stays_with_its_first_repository() {
    let router = registries(
        vec![
            registry("https://nova.laravel.com", None),
            registry("https://repo.packagist.org", None),
        ],
        &HashMap::new(),
    )
    .await;

    let name = PackageName::new("laravel/nova");
    router.found(&name, 1);
    router.found(&name, 0);

    assert!(eq(&router.route(&name)[0], &router.fallback[1]));
}

// ---------------------------------------------------------------------------
// Settling a clash between a repository and a source
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_repository_source_yields_to_a_self_pin() {
    let tmp = TempDir::new().unwrap();
    package(tmp.path(), "packages/local", "acme/local", "1.0.0");

    let (router, _) = router(
        tmp.path(),
        vec![
            path("packages/local"),
            registry("https://nova.laravel.com", Some("nova")),
        ],
        &sources("acme/local", repository("nova")),
    )
    .await;

    // Index 0 is the path repository. Naming a whole endpoint is the vaguer of
    // the two statements, so the local checkout keeps the package.
    let local = PackageName::new("acme/local");
    assert_eq!(router.sources.get(&local), Some(&0));
}

#[tokio::test]
async fn a_path_source_replaces_a_self_pin() {
    let tmp = TempDir::new().unwrap();
    package(tmp.path(), "packages/local", "acme/local", "1.0.0");
    package(tmp.path(), "other/local", "acme/local", "2.0.0");

    let source = Source::Path {
        path: "other/local".to_owned(),
        version: None,
    };
    let pinned = sources("acme/local", source);
    let (router, _) = router(tmp.path(), vec![path("packages/local")], &pinned).await;

    // Both directories declare acme/local. Unlike naming an endpoint, a path
    // source is a statement about this one package, so it wins: the version
    // says which directory answered.
    let local = PackageName::new("acme/local");
    let versions = router.route(&local)[0].versions(&local).await.unwrap();
    assert_eq!(
        versions.iter().map(|v| v.to_string()).collect::<Vec<_>>(),
        ["2.0"],
    );
}

// ---------------------------------------------------------------------------
// A source Phast cannot fetch from
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_vcs_repository_on_an_unsupported_host_is_skipped_with_a_warning() {
    let tmp = TempDir::new().unwrap();
    let (router, warnings) = router(
        tmp.path(),
        vec![
            PackageProvider::Vcs {
                url: UNSUPPORTED,
                name: Some("internal"),
            },
            registry("https://repo.packagist.org", None),
        ],
        &HashMap::new(),
    )
    .await;

    // Skipped rather than fatal: a manifest may carry a repository for a host
    // Phast has no driver for, and the rest of the project still installs.
    assert_eq!(router.providers.len(), 1);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains(UNSUPPORTED), "{warnings:?}");
    assert!(warnings[0].contains("unsupported host"), "{warnings:?}");
}

#[tokio::test]
async fn a_git_source_on_an_unsupported_host_is_skipped_with_a_warning() {
    let tmp = TempDir::new().unwrap();
    let source = Source::Git {
        url: UNSUPPORTED.to_owned(),
        reference: GitRef::Default,
    };
    let (router, warnings) = router(
        tmp.path(),
        vec![registry("https://repo.packagist.org", None)],
        &sources("acme/widget", source),
    )
    .await;

    assert!(router.sources.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("unsupported host"), "{warnings:?}");
}

#[tokio::test]
async fn an_inline_source_on_an_unsupported_host_is_skipped_with_a_warning() {
    let inline = Source::Package {
        package: serde_json::json!({
            "name": "acme/corpus",
            "version": "1.0.0",
            "source": { "type": "git", "url": UNSUPPORTED, "reference": "abc" },
        }),
    };
    let tmp = TempDir::new().unwrap();

    let (router, warnings) = router(
        tmp.path(),
        vec![registry("https://repo.packagist.org", None)],
        &sources("acme/corpus", inline),
    )
    .await;

    // The dist is synthesized from the source host, so a host with no driver
    // leaves nothing to install even though the spec is otherwise complete.
    assert!(router.sources.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("acme/corpus"), "{warnings:?}");
}

#[tokio::test]
async fn a_path_repository_that_cannot_be_read_stops_the_resolve() {
    let tmp = TempDir::new().unwrap();
    let mut router = RegistryRouter::new(HttpClient::new().unwrap(), Cache::default_root());
    let outcome = router
        .resolve(
            tmp.path(),
            vec![path("does/not/exist")],
            &HashMap::new(),
            options(),
            |_| {},
        )
        .await;

    // Unlike an unsupported host, this one is the user naming a directory that
    // is not there, so it is reported rather than quietly dropped.
    let error = format!("{:#}", outcome.expect_err("a missing path repo must fail"));
    assert!(error.contains("does/not/exist"), "{error}");
}

// ---------------------------------------------------------------------------
// Deriving the filters from a manifest
// ---------------------------------------------------------------------------

#[test]
fn the_floors_and_the_stability_overrides_come_from_the_manifest() {
    let toml = indoc! {r#"
        [require]
        "vendor/dev-pkg" = "^2.0@dev"
        "vendor/stable"  = "^1.0"

        [require-dev]
        "vendor/beta-pkg" = "^3.0@beta"

        [config]
        minimum-stability   = "RC"
        minimum-package-age = "24h"
    "#};
    let manifest = Manifest::from_phast_toml(toml).unwrap();
    let options = RegistryOptions::from_manifest(&manifest);

    // Both global floors are read from [config].
    assert_eq!(options.minimum_stability, Stability::RC);
    assert_eq!(
        options.minimum_package_age,
        Some(std::time::Duration::from_secs(24 * 60 * 60)),
    );

    // Only flagged require / require-dev entries land in the override map.
    assert_eq!(
        options.stability_overrides,
        HashMap::from([
            (PackageName::new("vendor/dev-pkg"), Stability::Dev),
            (PackageName::new("vendor/beta-pkg"), Stability::Beta),
        ]),
    );
}

#[test]
fn an_unstable_version_named_in_a_requirement_lowers_its_floor() {
    let toml = indoc! {r#"
        [require]
        "vendor/branch"  = "dev-master"
        "vendor/beta"    = "1.0.0-beta2"
        "vendor/flagged" = "dev-master@beta"
        "vendor/stable"  = "^1.0"
    "#};
    let manifest = Manifest::from_phast_toml(toml).unwrap();
    let options = RegistryOptions::from_manifest(&manifest);

    // The explicit flag wins over the branch it is written on.
    assert_eq!(
        options.stability_overrides,
        HashMap::from([
            (PackageName::new("vendor/branch"), Stability::Dev),
            (PackageName::new("vendor/beta"), Stability::Beta),
            (PackageName::new("vendor/flagged"), Stability::Beta),
        ]),
    );
}

#[test]
fn an_implied_stability_never_tightens_the_project_floor() {
    let toml = indoc! {r#"
        [require]
        "vendor/beta"    = "1.0.0-beta2"
        "vendor/flagged" = "^1.0@beta"

        [config]
        minimum-stability = "dev"
    "#};
    let manifest = Manifest::from_phast_toml(toml).unwrap();
    let options = RegistryOptions::from_manifest(&manifest);

    // Under a dev floor, `1.0.0-beta2` keeps its package at dev rather than
    // dropping its dev releases. An explicit flag still sets the floor outright.
    assert_eq!(
        options.stability_overrides,
        HashMap::from([(PackageName::new("vendor/flagged"), Stability::Beta)]),
    );
}
