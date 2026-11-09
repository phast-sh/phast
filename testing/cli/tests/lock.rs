//! The `phast lock` command surface.
//!
//! `lock` resolves and writes `phast.lock` but downloads nothing, so the distinguishing
//! assertion throughout is that `vendor/` never appears. Resolution runs against a mock
//! registry via `--registry-url`, so nothing here touches the network.

mod common;

use common::project::Project;
use common::registry::MockRegistry;

use indoc::{formatdoc, indoc};
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::json;

const MANIFEST: &str = indoc! {r#"
    [package]
    name = "acme/my-project"
    type = "project"

    [require]
    "vendor/alpha" = "^1.0"

    [require-dev]
    "vendor/dev-tool" = "^2.0"
"#};

/// A project asking for one real package and one that exists nowhere.
const MANIFEST_WITH_UNKNOWN: &str = indoc! {r#"
    [package]
    name = "acme/my-project"
    type = "project"

    [require]
    "vendor/alpha"      = "^1.0"
    "acme/not-real-xyz" = "^1.0"
"#};

/// A project requiring an interface no registry carries, plus a package that provides it.
const MANIFEST_VIRTUAL: &str = indoc! {r#"
    [package]
    name = "acme/my-project"
    type = "project"

    [require]
    "vendor/logger"           = "^1.0"
    "psr/log-implementation"  = "^3.0"
"#};

/// A project requiring a package that another one replaces.
const MANIFEST_REPLACED: &str = indoc! {r#"
    [package]
    name = "acme/my-project"
    type = "project"

    [require]
    "vendor/framework" = "^1.0"
    "vendor/legacy"    = "^1.0"
"#};

/// A project whose only requirement pulls in a virtual name transitively.
const MANIFEST_TRANSITIVE_VIRTUAL: &str = indoc! {r#"
    [package]
    name = "acme/my-project"
    type = "project"

    [require]
    "vendor/needs-a-logger" = "^1.0"
    "vendor/logger"         = "^1.0"
"#};

/// A project requiring, at any version, a package published only as a prerelease.
const MANIFEST_PRERELEASE_ONLY: &str = indoc! {r#"
    [package]
    name = "acme/my-project"
    type = "project"

    [require]
    "vendor/alpha-only" = "*"
"#};

/// Refused by `secure-http` before any connection is made, so the test stays offline.
/// A loopback host would be exempt, which is why the mock registry gets away with `http://`.
const REFUSED_HOST: &str = "http://repo.example.invalid";

/// Nothing listens on port 1, so the connection is refused at once.
const UNREACHABLE_HOST: &str = "https://127.0.0.1:1";

/// A publish time inside any `minimum-package-age`, without depending on the clock.
const PUBLISHED_TOO_RECENTLY: &str = "2999-01-01T00:00:00+00:00";

/// A publish time outside any `minimum-package-age` a project would set.
const PUBLISHED_LONG_AGO: &str = "2000-01-01T00:00:00+00:00";

/// A project requiring one package and one dev package.
fn project() -> Project {
    Project::new().manifest(MANIFEST)
}

/// A registry serving both packages the manifest asks for.
async fn registry() -> MockRegistry {
    let registry = MockRegistry::start().await;
    registry.mount("vendor/alpha", &["1.0.0"], json!({})).await;
    registry
        .mount("vendor/dev-tool", &["2.0.0"], json!({}))
        .await;
    registry
}

// ---------------------------------------------------------------------------
// Writing the lock
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lock_writes_the_lock_without_downloading() {
    let registry = registry().await;
    let project = project();

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .success();

    assert!(project.root().join("phast.lock").exists());
    assert!(
        !project.root().join("vendor").exists(),
        "lock must not download anything",
    );
}

#[tokio::test]
async fn lock_writes_no_cursor_codes_to_a_pipe() {
    let registry = registry().await;
    let project = project();

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .success()
        .stderr(contains("\x1b[?25").not());
}

#[tokio::test]
async fn lock_records_the_resolved_packages() {
    let registry = registry().await;
    let project = project();

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .success();

    let lock = project.read("phast.lock");
    assert!(lock.contains("vendor/alpha"), "missing dependency:\n{lock}");
    assert!(lock.contains("1.0.0"), "missing version:\n{lock}");
}

#[tokio::test]
async fn lock_includes_dev_dependencies() {
    let registry = registry().await;
    let project = project();

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .success();

    assert!(project.read("phast.lock").contains("vendor/dev-tool"));
}

#[tokio::test]
async fn lock_fails_without_a_manifest() {
    let project = Project::new();

    project
        .cmd()
        .arg("lock")
        .assert()
        .failure()
        .stderr(contains("neither phast.toml nor composer.json found"));
}

// ---------------------------------------------------------------------------
// Re-resolving only a stale lock
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lock_skips_resolution_when_already_fresh() {
    let registry = registry().await;
    let project = project();

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .success();

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .success()
        .stdout(contains("up to date"));
}

#[tokio::test]
async fn lock_refresh_re_resolves_a_fresh_lock() {
    let registry = registry().await;
    let project = project();

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .success();

    project
        .cmd()
        .args(["lock", "--refresh", "--registry-url", &registry.uri()])
        .assert()
        .success()
        .stdout(contains("up to date").not());
}

// ---------------------------------------------------------------------------
// Requirements no registry can satisfy
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lock_fails_when_a_requirement_exists_nowhere() {
    let registry = MockRegistry::start().await;
    registry.mount("vendor/alpha", &["1.0.0"], json!({})).await;

    let project = Project::new().manifest(MANIFEST_WITH_UNKNOWN);

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .failure()
        .stderr(contains("acme/not-real-xyz"));

    assert!(
        !project.root().join("phast.lock").exists(),
        "a lock was written for a manifest that cannot resolve",
    );
}

#[tokio::test]
async fn lock_fails_when_only_a_prerelease_version_exists() {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/alpha-only", &["1.0.0-alpha"], json!({}))
        .await;

    let project = Project::new().manifest(MANIFEST_PRERELEASE_ONLY);

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .failure()
        .stderr(contains("vendor/alpha-only"))
        .stderr(contains("minimum-stability"));
}

#[tokio::test]
async fn lock_fails_for_any_constraint_form() {
    for constraint in ["*", "^1.0", ">=1.0"] {
        let registry = MockRegistry::start().await;
        registry.mount("vendor/alpha", &["1.0.0"], json!({})).await;

        let project = Project::new().manifest(&formatdoc! {r#"
            [package]
            name = "acme/my-project"
            type = "project"

            [require]
            "vendor/alpha"      = "^1.0"
            "acme/not-real-xyz" = "{constraint}"
        "#});

        project
            .cmd()
            .args(["lock", "--registry-url", &registry.uri()])
            .assert()
            .failure()
            .stderr(contains("acme/not-real-xyz"));
    }
}

// ---------------------------------------------------------------------------
// Versions too recent for minimum-package-age
// ---------------------------------------------------------------------------

/// The only version is too recent, so the package looks as if no registry had it, and the
/// hint is what says otherwise.
#[tokio::test]
async fn lock_names_the_age_window_when_every_version_is_too_recent() {
    let registry = published(&[("1.0.0", PUBLISHED_TOO_RECENTLY)]).await;
    let project = requiring(r#""vendor/alpha" = "^1.0""#, Some("7d"));

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .failure()
        .stderr(contains("vendor/alpha"))
        .stderr(contains("was published 2999-01-01"))
        .stderr(contains("inside minimum-package-age"))
        .stderr(contains("not offered until 2999-01-08"));
}

/// An older version survives the filter but misses the constraint, so the solver reports
/// the range it could not meet, and the version that would have met it is named.
#[tokio::test]
async fn lock_names_the_age_window_when_only_a_too_recent_version_fits() {
    let registry = published(&[
        ("1.0.0", PUBLISHED_LONG_AGO),
        ("1.1.0", PUBLISHED_TOO_RECENTLY),
    ])
    .await;
    let project = requiring(r#""vendor/alpha" = "1.1.0""#, Some("7d"));

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .failure()
        .stderr(contains("inside minimum-package-age"));
}

/// The window working as configured is not an error: the older version is chosen.
#[tokio::test]
async fn lock_falls_back_to_an_older_version_without_a_hint() {
    let registry = published(&[
        ("1.0.0", PUBLISHED_LONG_AGO),
        ("1.1.0", PUBLISHED_TOO_RECENTLY),
    ])
    .await;
    let project = requiring(r#""vendor/alpha" = "^1.0""#, Some("7d"));

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .success()
        .stderr(contains("minimum-package-age").not());

    assert!(project.read("phast.lock").contains("version = \"1.0.0\""));
}

/// A version the window kept out of a package the failure does not name is left unsaid,
/// or every recent release in the project would pile onto an unrelated error.
#[tokio::test]
async fn lock_leaves_the_age_window_out_of_an_unrelated_failure() {
    let registry = published(&[
        ("1.0.0", PUBLISHED_LONG_AGO),
        ("1.1.0", PUBLISHED_TOO_RECENTLY),
    ])
    .await;
    let project = requiring(
        indoc! {r#"
            "vendor/alpha"      = "^1.0"
            "acme/not-real-xyz" = "^1.0"
        "#},
        Some("7d"),
    );

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .failure()
        .stderr(contains("acme/not-real-xyz"))
        .stderr(contains("minimum-package-age").not());
}

#[tokio::test]
async fn lock_names_no_age_window_when_none_is_set() {
    let registry = published(&[("1.0.0", PUBLISHED_TOO_RECENTLY)]).await;
    let project = requiring(r#""vendor/alpha" = "2.0.0""#, None);

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .failure()
        .stderr(contains("vendor/alpha"))
        .stderr(contains("minimum-package-age").not());
}

/// The first registry carries the package, so holding its only release back must not hand
/// the lookup to the next registry, which would install a same-named package instead.
#[tokio::test]
async fn lock_stops_at_the_registry_whose_releases_are_too_recent() {
    let owner = published(&[("1.0.0", PUBLISHED_TOO_RECENTLY)]).await;
    let other = published(&[("9.9.9", PUBLISHED_LONG_AGO)]).await;
    let project = chain(&owner, &other, Some("1d"));

    project
        .cmd()
        .arg("lock")
        .assert()
        .failure()
        .stderr(contains("inside minimum-package-age"));

    assert!(
        !project.root().join("phast.lock").exists(),
        "nothing may be locked from the later registry",
    );
}

#[tokio::test]
async fn lock_takes_the_first_registry_without_an_age_window() {
    let owner = published(&[("1.0.0", PUBLISHED_TOO_RECENTLY)]).await;
    let other = published(&[("9.9.9", PUBLISHED_LONG_AGO)]).await;
    let project = chain(&owner, &other, None);

    project.cmd().arg("lock").assert().success();

    let lock: toml::Value = toml::from_str(&project.read("phast.lock")).expect("lock is TOML");
    assert_eq!(lock["packages"][0]["version"].as_str(), Some("1.0.0"));
}

/// A registry serving `vendor/alpha` at each version in `versions`, published at the time
/// paired with it.
async fn published(versions: &[(&str, &str)]) -> MockRegistry {
    let versions: Vec<(&str, serde_json::Value)> = versions
        .iter()
        .map(|(version, time)| (*version, json!({ "time": time })))
        .collect();

    let registry = MockRegistry::start().await;
    registry.mount_each("vendor/alpha", &versions).await;
    registry
}

/// A project with `requires` as its `[require]` entries, and `minimum-package-age` set to
/// `age` when given.
fn requiring(requires: &str, age: Option<&str>) -> Project {
    let config = age
        .map(|age| {
            formatdoc! {r#"
                [config]
                minimum-package-age = "{age}"
            "#}
        })
        .unwrap_or_default();

    Project::new().manifest(&formatdoc! {r#"
        [package]
        name = "acme/my-project"
        type = "project"

        [require]
        {requires}

        {config}
    "#})
}

/// A project requiring any `vendor/alpha` from `owner` first and `other` second, with
/// `minimum-package-age` set to `age` when given.
fn chain(owner: &MockRegistry, other: &MockRegistry, age: Option<&str>) -> Project {
    let config = age
        .map(|age| {
            formatdoc! {r#"
                [config]
                minimum-package-age = "{age}"
            "#}
        })
        .unwrap_or_default();

    Project::new().manifest(&formatdoc! {r#"
        [package]
        name = "acme/my-project"
        type = "project"

        [require]
        "vendor/alpha" = "*"

        {config}
        [[repositories]]
        type = "composer"
        url  = "{owner}"

        [[repositories]]
        type = "composer"
        url  = "{other}"
    "#, owner = owner.uri(), other = other.uri()})
}

// ---------------------------------------------------------------------------
// Repositories that cannot be used
// ---------------------------------------------------------------------------

#[test]
fn lock_names_a_repository_secure_http_refuses() {
    let project = repositories(&[REFUSED_HOST]);

    project
        .cmd()
        .arg("lock")
        .assert()
        .failure()
        .stderr(contains("insecure HTTP to http://repo.example.invalid"))
        .stderr(contains("secure-http = false"));
}

#[test]
fn lock_names_a_repository_it_cannot_reach() {
    let project = repositories(&[UNREACHABLE_HOST]);

    project
        .cmd()
        .arg("lock")
        .assert()
        .failure()
        .stderr(contains(
            "could not reach the registry at https://127.0.0.1:1",
        ));
}

/// Moving on to the next repository would install whatever it calls `acme/private`
/// whenever the first one is down.
#[tokio::test]
async fn lock_stops_at_a_failing_repository_rather_than_trying_the_next() {
    let registry = MockRegistry::start().await;
    registry.mount("acme/private", &["1.0.0"], json!({})).await;

    let project = repositories(&[UNREACHABLE_HOST, &registry.uri()]);

    project
        .cmd()
        .arg("lock")
        .assert()
        .failure()
        .stderr(contains(
            "could not reach the registry at https://127.0.0.1:1",
        ));

    assert!(!project.root().join("phast.lock").exists());
}

/// The refusal used to show only when a lock existed; without one it read as a missing
/// package.
#[test]
fn lock_names_a_registry_url_secure_http_refuses() {
    let project = project();

    project
        .cmd()
        .args(["lock", "--registry-url", REFUSED_HOST])
        .assert()
        .failure()
        .stderr(contains("insecure HTTP to http://repo.example.invalid"))
        .stderr(contains("secure-http = false"));
}

/// A project requiring `acme/private` from the repositories at `urls`, in that order, with
/// Packagist off.
fn repositories(urls: &[&str]) -> Project {
    let repositories: String = urls
        .iter()
        .map(|url| {
            formatdoc! {r#"
            [[repositories]]
            type = "composer"
            url  = "{url}"
        "#}
        })
        .collect();

    Project::new().manifest(&formatdoc! {r#"
        [package]
        name = "acme/my-project"
        type = "project"

        [require]
        "acme/private" = "^1.0"

        {repositories}

        [[repositories]]
        type = "packagist"
    "#})
}

// ---------------------------------------------------------------------------
// Names carried by something other than a registry
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lock_resolves_a_virtual_package_its_provider_carries() {
    let registry = MockRegistry::start().await;
    registry
        .mount(
            "vendor/logger",
            &["1.0.0"],
            json!({"provide": { "psr/log-implementation": "3.0.0" } }),
        )
        .await;

    let project = Project::new().manifest(MANIFEST_VIRTUAL);

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .success();

    // The interface has no artifact, so only its provider is recorded.
    // Asserted on the entry names rather than the text: the name also
    // appears in the provider's own `provide` table, which is exactly
    // where it belongs.
    let locked = locked(&project);
    assert!(
        locked.contains(&"vendor/logger".to_owned()),
        "provider missing: {locked:?}",
    );
    assert!(
        !locked.contains(&"psr/log-implementation".to_owned()),
        "a virtual package was recorded as installable: {locked:?}",
    );
}

#[tokio::test]
async fn lock_resolves_a_package_its_replacer_carries() {
    let registry = MockRegistry::start().await;
    registry
        .mount(
            "vendor/framework",
            &["1.0.0"],
            json!({ "replace": { "vendor/legacy": "1.0.0" } }),
        )
        .await;

    let project = Project::new().manifest(MANIFEST_REPLACED);

    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .success();

    let locked = locked(&project);
    assert!(
        locked.contains(&"vendor/framework".to_owned()),
        "replacer missing: {locked:?}",
    );
    assert!(
        !locked.contains(&"vendor/legacy".to_owned()),
        "a replaced package was recorded as installable: {locked:?}",
    );
}

#[tokio::test]
async fn lock_resolves_a_virtual_package_required_by_a_dependency() {
    let registry = MockRegistry::start().await;
    registry
        .mount(
            "vendor/needs-a-logger",
            &["1.0.0"],
            json!({ "require": { "psr/log-implementation": "^3.0" } }),
        )
        .await;
    registry
        .mount(
            "vendor/logger",
            &["1.0.0"],
            json!({ "provide": { "psr/log-implementation": "3.0.0" } }),
        )
        .await;

    let project = Project::new().manifest(MANIFEST_TRANSITIVE_VIRTUAL);

    // The virtual name is required by a dependency rather than the manifest,
    // so the post-solve check must not look at it at all.
    project
        .cmd()
        .args(["lock", "--registry-url", &registry.uri()])
        .assert()
        .success();

    let locked = locked(&project);
    assert!(
        locked.contains(&"vendor/needs-a-logger".to_owned()),
        "missing: {locked:?}",
    );
    assert!(
        locked.contains(&"vendor/logger".to_owned()),
        "missing: {locked:?}",
    );
}

/// The names of the production packages in `project`'s lock, in the order it lists them.
#[rustfmt::skip]
fn locked(project: &Project) -> Vec<String> {
    let lock: toml::Value = toml::from_str(&project.read("phast.lock"))
        .expect("lock is TOML");

    let packages = lock["packages"]
        .as_array()
        .expect("packages is an array");

    packages
        .iter()
        .map(|package| package["name"].as_str().expect("name is a string").to_owned())
        .collect()
}
