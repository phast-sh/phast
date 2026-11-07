mod common;

use common::project::Project;
use common::registry::MockRegistry;

use indoc::{formatdoc, indoc};
use predicates::str::contains;
use serde_json::json;

#[rustfmt::skip]
fn project(registry: Option<&str>) -> Project {
    let repositories = registry.map_or_else(String::new, |uri| {
        formatdoc! {r#"
            [[repositories]]
            type = "composer"
            url  = "{uri}"

            [[repositories]]
            type = "packagist"
        "#}
    });

    Project::new().manifest(
        &formatdoc! {r#"
            [package]
            name = "acme/my-project"
            type = "project"

            [require]
            "vendor/package-a" = "^1.0"

            {repositories}
        "#}
    )
}

#[rustfmt::skip]
fn workspace() -> Project {
    let project = Project::new().manifest(
        indoc! {r#"
            [workspace]
            members = ["packages/*"]
        "#}
    );

    project.write("packages/api/phast.toml",
        indoc! {r#"
            [package]
            name    = "acme/api"
            version = "1.0.0"

            [require]
            "vendor/package-a" = "^1.0"
        "#}
    )
}

#[rustfmt::skip]
fn workspace_with_dependency(registry: &str) -> Project {
    let project = Project::new().manifest(
        &formatdoc! {r#"
            [workspace]
            members = ["packages/*"]

            [workspace.dependencies]
            "vendor/package-a" = "^1.0"

            [[repositories]]
            type = "composer"
            url  = "{registry}"

            [[repositories]]
            type = "packagist"
        "#}
    );

    project.write("packages/api/phast.toml",
        indoc! {r#"
            [package]
            name    = "acme/api"
            version = "1.0.0"
        "#}
    )
}

/// Two members, neither pinned by the root, so a sibling's version has to come from the
/// sibling. Offline by construction: nothing here is published anywhere.
#[rustfmt::skip]
fn siblings() -> Project {
    let project = Project::new().manifest(
        indoc! {r#"
            [workspace]
            members = ["packages/*"]
        "#}
    );

    project
        .write("packages/app/phast.toml",
            indoc! {r#"
                [package]
                name    = "acme/app"
                version = "1.0.0"
            "#}
        )
        .write("packages/other/phast.toml",
            indoc! {r#"
                [package]
                name    = "acme/other"
                version = "1.2.0"
            "#}
        )
}

#[test]
fn add_package_to_require() {
    let project = project(None);

    project
        .cmd()
        .args(["add", "vendor/package-b:^2.0", "--no-install"])
        .assert()
        .success();

    let toml = project.to_toml();
    assert_eq!(toml["require"]["vendor/package-b"].as_str(), Some("^2.0"));
}

#[test]
fn add_package_to_require_dev() {
    let project = project(None);

    project
        .cmd()
        .args(["add", "--dev", "vendor/package-b:^2.0", "--no-install"])
        .assert()
        .success();

    let toml = project.to_toml();
    assert_eq!(
        toml["require-dev"]["vendor/package-b"].as_str(),
        Some("^2.0")
    );
}

#[test]
fn add_does_not_affect_other_sections() {
    let project = project(None);

    project
        .cmd()
        .args(["add", "vendor/package-b:^2.0", "--no-install"])
        .assert()
        .success();

    let toml = project.to_toml();
    assert_eq!(toml["require"]["vendor/package-a"].as_str(), Some("^1.0"));
    assert!(toml.get("require-dev").is_none());
}

#[test]
fn add_fails_when_package_already_in_opposite_section() {
    let project = project(None);

    project
        .cmd()
        .args(["add", "--dev", "vendor/package-a:^1.0", "--no-install"])
        .assert()
        .failure();
}

#[test]
fn add_records_a_platform_package_as_any_version() {
    let project = project(None);

    project
        .cmd()
        .args(["add", "ext-json", "php", "--no-install"])
        .assert()
        .success();

    let toml = project.to_toml();
    assert_eq!(toml["require"]["ext-json"].as_str(), Some("*"));
    assert_eq!(toml["require"]["php"].as_str(), Some("*"));
}

#[tokio::test]
async fn add_records_a_provided_package_as_any_version() {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/package-a", &["1.0.0"], json!({}))
        .await;
    registry
        .mount(
            "vendor/logger",
            &["1.0.0"],
            json!({ "provide": { "psr/log-implementation": "3.0.0" } }),
        )
        .await;

    let project = project(Some(&registry.uri()));

    project
        .cmd()
        .args([
            "add",
            "vendor/logger",
            "psr/log-implementation",
            "--no-install",
        ])
        .assert()
        .success();

    // Carried by the logger rather than installed, so there is no version to derive a
    // caret from -- the same reason a platform package takes `*`.
    let toml = project.to_toml();
    assert_eq!(toml["require"]["vendor/logger"].as_str(), Some("^1.0"));
    assert_eq!(
        toml["require"]["psr/log-implementation"].as_str(),
        Some("*"),
    );
}

#[tokio::test]
async fn add_with_a_constraint_asks_no_registry() {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/package-a", &["1.0.0"], json!({}))
        .await;

    let project = project(Some(&registry.uri()));

    project
        .cmd()
        .args(["add", "vendor/package-b:^2.0", "--no-install"])
        .assert()
        .success();

    let toml = project.to_toml();
    assert_eq!(toml["require"]["vendor/package-b"].as_str(), Some("^2.0"));
    assert_eq!(
        registry.requests().await,
        0,
        "a constraint was given, so no version had to be asked for",
    );
}

#[tokio::test]
async fn add_derives_a_constraint_for_require_dev() {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/package-a", &["1.0.0"], json!({}))
        .await;
    registry
        .mount("vendor/package-b", &["2.3.4"], json!({}))
        .await;

    let project = project(Some(&registry.uri()));

    project
        .cmd()
        .args(["add", "--dev", "vendor/package-b", "--no-install"])
        .assert()
        .success();

    let toml = project.to_toml();
    assert_eq!(
        toml["require-dev"]["vendor/package-b"].as_str(),
        Some("^2.3")
    );
    assert!(toml["require"].get("vendor/package-b").is_none());
}

#[tokio::test]
async fn add_keeps_given_constraints_and_chooses_the_rest() {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/package-a", &["1.0.0"], json!({}))
        .await;
    registry
        .mount("vendor/package-b", &["2.3.4"], json!({}))
        .await;
    registry
        .mount("vendor/package-c", &["4.5.6"], json!({}))
        .await;

    let project = project(Some(&registry.uri()));

    project
        .cmd()
        .args([
            "add",
            "vendor/package-b:^2.0",
            "vendor/package-c",
            "--no-install",
        ])
        .assert()
        .success();

    let toml = project.to_toml();
    assert_eq!(toml["require"]["vendor/package-b"].as_str(), Some("^2.0"));
    assert_eq!(toml["require"]["vendor/package-c"].as_str(), Some("^4.5"));
}

#[tokio::test]
async fn add_derives_a_caret_from_the_resolved_version() {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/package-a", &["1.0.0"], json!({}))
        .await;
    registry
        .mount("vendor/package-b", &["2.3.4"], json!({}))
        .await;

    let project = project(Some(&registry.uri()));

    project
        .cmd()
        .args(["add", "vendor/package-b", "--no-install"])
        .assert()
        .success();

    let toml = project.to_toml();
    assert_eq!(toml["require"]["vendor/package-b"].as_str(), Some("^2.3"));

    let lock = project.read("phast.lock");
    assert!(
        lock.contains("2.3.4"),
        "lock does not carry the exact version:\n{lock}"
    );
}

#[tokio::test]
async fn add_resolves_the_requirements_of_what_it_adds() {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/package-a", &["1.0.0", "2.0.0"], json!({}))
        .await;
    registry
        .mount(
            "vendor/package-b",
            &["1.5.0"],
            json!({ "require": { "vendor/package-a": "^1.0" } }),
        )
        .await;

    let project = project(Some(&registry.uri()));

    project
        .cmd()
        .args(["add", "vendor/package-b"])
        .assert()
        .success();

    let toml = project.to_toml();
    assert_eq!(toml["require"]["vendor/package-b"].as_str(), Some("^1.5"));

    // package-a has a 2.0.0, and the project's own `^1.0` is what keeps it at 1.0.0.
    let lock = project.read("phast.lock");
    assert!(
        lock.contains("1.0.0"),
        "package-a was not held at 1.0.0:\n{lock}"
    );
    assert!(!lock.contains("2.0.0"), "package-a moved to 2.0.0:\n{lock}");
}

#[tokio::test]
async fn add_leaves_a_lock_that_install_reads_as_fresh() {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/package-a", &["1.0.0"], json!({}))
        .await;
    registry
        .mount("vendor/package-b", &["2.3.4"], json!({}))
        .await;

    let project = project(Some(&registry.uri()));

    project
        .cmd()
        .args(["add", "vendor/package-b", "--no-install"])
        .assert()
        .success();

    project
        .cmd()
        .arg("install")
        .assert()
        .success()
        .stdout(contains("Lock file is up to date"));
}

#[tokio::test]
async fn add_without_a_constraint_writes_nothing_when_unresolvable() {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/package-a", &["1.0.0"], json!({}))
        .await;

    let project = project(Some(&registry.uri()));
    let manifest = project.read("phast.toml");

    project
        .cmd()
        .args(["add", "vendor/nowhere", "--no-install"])
        .assert()
        .failure();

    assert_eq!(project.read("phast.toml"), manifest);
}

#[tokio::test]
async fn add_installs_into_a_project_with_a_fresh_lock() {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/package-a", &["1.0.0"], json!({}))
        .await;
    registry
        .mount("vendor/package-b", &["2.0.0"], json!({}))
        .await;

    let project = project(Some(&registry.uri()));

    // Leaves a lock that is fresh for the manifest as it stands.
    project.cmd().arg("install").assert().success();

    project
        .cmd()
        .args(["add", "vendor/package-b:^2.0"])
        .assert()
        .success();

    let lock = project.read("phast.lock");
    assert!(
        lock.contains("vendor/package-b"),
        "package-b missing from the lock:\n{lock}"
    );
    assert!(
        project.root().join("vendor/vendor/package-b").is_dir(),
        "package-b was recorded but never installed",
    );
}

#[tokio::test]
async fn add_inherits_a_constraint_the_root_pins() {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/package-a", &["1.0.0"], json!({}))
        .await;

    let project = workspace_with_dependency(&registry.uri());

    project
        .cmd()
        .args(["add", "--member", "packages/api"])
        .args(["vendor/package-a", "--no-install"])
        .assert()
        .success();

    let member: toml::Value =
        toml::from_str(&project.read("packages/api/phast.toml")).expect("member is valid TOML");
    let require = member["require"].clone();

    assert_eq!(
        require["vendor/package-a"]["workspace"].as_bool(),
        Some(true),
        "the member spelled a constraint instead of inheriting",
    );

    // Which is the whole point: a member that never spells the constraint cannot spell it
    // differently from the root.
    project.cmd().arg("workspace").assert().success();

    assert_eq!(
        registry.requests().await,
        0,
        "the root pinned it, so no version had to be chosen",
    );
}

#[tokio::test]
async fn add_derives_a_constraint_the_root_does_not_pin() {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/package-b", &["2.3.4"], json!({}))
        .await;

    let project = workspace_with_dependency(&registry.uri());

    project
        .cmd()
        .args(["add", "--member", "packages/api"])
        .args(["vendor/package-b", "--no-install"])
        .assert()
        .success();

    let member: toml::Value =
        toml::from_str(&project.read("packages/api/phast.toml")).expect("member is valid TOML");
    let require = member["require"].clone();

    assert_eq!(require["vendor/package-b"].as_str(), Some("^2.3"),);

    // Also what keeps the zero-request assertion above honest: the same fixture reaches
    // this registry when a version actually has to be chosen.
    assert!(
        registry.requests().await > 0,
        "nothing was asked of the registry, so counting requests proves nothing",
    );
}

#[tokio::test]
async fn add_refuses_a_constraint_the_root_pins_differently() {
    let registry = MockRegistry::start().await;
    let project = workspace_with_dependency(&registry.uri());
    let member = project.read("packages/api/phast.toml");

    project
        .cmd()
        .args(["add", "--member", "packages/api"])
        .args(["vendor/package-a:^2.0", "--no-install"])
        .assert()
        .failure()
        .stderr(contains("^1.0"));

    assert_eq!(
        project.read("packages/api/phast.toml"),
        member,
        "the member manifest was written to",
    );
}

#[tokio::test]
async fn add_normalises_a_constraint_matching_the_root() {
    let registry = MockRegistry::start().await;
    let project = workspace_with_dependency(&registry.uri());

    project
        .cmd()
        .args(["add", "--member", "packages/api"])
        .args(["vendor/package-a:^1.0", "--no-install"])
        .assert()
        .success();

    let member: toml::Value =
        toml::from_str(&project.read("packages/api/phast.toml")).expect("member is valid TOML");
    let require = member["require"].clone();

    assert_eq!(
        require["vendor/package-a"]["workspace"].as_bool(),
        Some(true),
        "a constraint agreeing with the root was recorded as its own copy",
    );
}

#[test]
fn add_at_a_workspace_root_is_refused() {
    let project = workspace();
    let manifest = project.read("phast.toml");

    project
        .cmd()
        .args(["add", "vendor/package-b:^2.0", "--no-install"])
        .assert()
        .failure()
        .stderr(contains("--member"));

    assert_eq!(
        project.read("phast.toml"),
        manifest,
        "the root manifest was written to",
    );
}

#[test]
fn add_dev_at_a_workspace_root_is_refused() {
    let project = workspace();
    let manifest = project.read("phast.toml");

    project
        .cmd()
        .args(["add", "--dev", "vendor/package-b:^2.0", "--no-install"])
        .assert()
        .failure()
        .stderr(contains("--member"));

    assert_eq!(
        project.read("phast.toml"),
        manifest,
        "the root manifest was written to",
    );
}

#[test]
fn add_fails_without_phast_toml() {
    let project = Project::new();

    project
        .cmd()
        .args(["add", "vendor/package-b:^2.0", "--no-install"])
        .assert()
        .failure();
}

#[test]
fn add_reports_a_missing_manifest_before_looking_for_a_workspace() {
    Project::new()
        .cmd()
        .args(["add", "psr/log"])
        .assert()
        .failure()
        .stderr(contains("phast.toml not found"))
        .stderr(contains("phast migrate"))
        .stderr(contains("phast init"));
}

#[tokio::test]
async fn add_resolves_a_version_from_the_named_registry() {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/package-b", &["2.3.0"], json!({}))
        .await;

    let project = Project::new().manifest(indoc! {r#"
        [package]
        name = "acme/my-project"
        type = "project"
    "#});

    project
        .cmd()
        .args([
            "add",
            "vendor/package-b",
            "--no-install",
            "--registry-url",
            &registry.uri(),
        ])
        .assert()
        .success();

    let manifest = project.read("phast.toml");
    assert!(
        manifest.contains(r#""vendor/package-b" = "^2.3""#),
        "the version was not resolved from the named registry:\n{manifest}",
    );
}

// ---------------------------------------------------------------------------
// Adding one workspace member to another
// ---------------------------------------------------------------------------

#[tokio::test]
async fn add_derives_a_sibling_constraint_from_the_member() {
    let project = siblings();

    project
        .cmd()
        .args([
            "add",
            "--member",
            "packages/app",
            "acme/other",
            "--no-install",
        ])
        .assert()
        .success();

    assert!(
        project
            .read("packages/app/phast.toml")
            .contains(r#""acme/other" = "^1.2""#),
        "the sibling's version was not derived:\n{}",
        project.read("packages/app/phast.toml"),
    );
}

#[tokio::test]
async fn add_refuses_a_sibling_that_declares_no_version() {
    let project = siblings().write(
        "packages/anon/phast.toml",
        indoc! {r#"
            [package]
            name = "acme/anon"
        "#},
    );

    project
        .cmd()
        .args([
            "add",
            "--member",
            "packages/app",
            "acme/anon",
            "--no-install",
        ])
        .assert()
        .failure()
        .stderr(contains(
            "workspace member 'acme/anon' declares no [package].version",
        ));
}

#[tokio::test]
async fn add_keeps_a_constraint_given_for_a_sibling() {
    let project = siblings();

    project
        .cmd()
        .args([
            "add",
            "--member",
            "packages/app",
            "acme/other:^1.0",
            "--no-install",
        ])
        .assert()
        .success();

    assert!(
        project
            .read("packages/app/phast.toml")
            .contains(r#""acme/other" = "^1.0""#),
    );
}

#[tokio::test]
async fn add_inherits_a_sibling_the_root_pins() {
    let project = Project::new()
        .manifest(indoc! {r#"
            [workspace]
            members = ["packages/*"]

            [workspace.dependencies]
            "acme/other" = "^1.0"
        "#})
        .write(
            "packages/app/phast.toml",
            indoc! {r#"
                [package]
                name    = "acme/app"
                version = "1.0.0"
            "#},
        )
        .write(
            "packages/other/phast.toml",
            indoc! {r#"
                [package]
                name    = "acme/other"
                version = "1.2.0"
            "#},
        );

    project
        .cmd()
        .args([
            "add",
            "--member",
            "packages/app",
            "acme/other",
            "--no-install",
        ])
        .assert()
        .success();

    assert!(
        project
            .read("packages/app/phast.toml")
            .contains(r#""acme/other" = { workspace = true }"#),
        "the root's pin did not win:\n{}",
        project.read("packages/app/phast.toml"),
    );
}

#[tokio::test]
async fn add_takes_a_sibling_and_a_registry_package_together() {
    let registry = MockRegistry::start().await;
    registry
        .mount("vendor/package-b", &["2.3.0"], json!({}))
        .await;

    let project = siblings();

    project
        .cmd()
        .args([
            "add",
            "--member",
            "packages/app",
            "acme/other",
            "vendor/package-b",
            "--no-install",
            "--registry-url",
            &registry.uri(),
        ])
        .assert()
        .success();

    let manifest = project.read("packages/app/phast.toml");
    assert!(
        manifest.contains(r#""acme/other" = "^1.2""#),
        "sibling:\n{manifest}"
    );
    assert!(
        manifest.contains(r#""vendor/package-b" = "^2.3""#),
        "registry package:\n{manifest}",
    );
}
