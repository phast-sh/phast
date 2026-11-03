mod common;

use common::project::Project;

use indoc::indoc;

// Full composer.json covering all sections migrate is expected to translate.
const COMPOSER_JSON: &str = indoc! {r#"
    {
        "name": "acme/my-project",
        "description": "A test project",
        "type": "project",
        "require": {
            "php": "^8.2",
            "vendor/package-a": "^1.0"
        },
        "require-dev": {
            "vendor/package-b": "^2.0"
        },
        "autoload": {
            "psr-4": { "Acme\\": "src/" },
            "files": ["bootstrap/helpers.php"]
        },
        "autoload-dev": {
            "psr-4": { "Acme\\Tests\\": "tests/" }
        },
        "minimum-stability": "dev",
        "prefer-stable": true,
        "config": {}
    }"#};

/// A project holding only a composer.json to migrate from.
fn project(composer_json: &str) -> Project {
    Project::new().write("composer.json", composer_json)
}

#[test]
fn migrate_full_structure() {
    let phast_toml = migrate(COMPOSER_JSON);

    // [package]
    assert_eq!(
        phast_toml["package"]["name"].as_str(),
        Some("acme/my-project")
    );
    assert_eq!(
        phast_toml["package"]["description"].as_str(),
        Some("A test project")
    );
    assert_eq!(phast_toml["package"]["type"].as_str(), Some("project"));

    // [require]
    assert_eq!(phast_toml["require"]["php"].as_str(), Some("^8.2"));
    assert_eq!(
        phast_toml["require"]["vendor/package-a"].as_str(),
        Some("^1.0")
    );

    // [require-dev]
    assert_eq!(
        phast_toml["require-dev"]["vendor/package-b"].as_str(),
        Some("^2.0")
    );

    // [autoload]
    assert_eq!(
        phast_toml["autoload"]["psr-4"]["Acme\\"]
            .as_array()
            .unwrap(),
        &vec![toml::Value::String("src/".into())]
    );
    assert_eq!(
        phast_toml["autoload"]["files"].as_array().unwrap(),
        &vec![toml::Value::String("bootstrap/helpers.php".into())]
    );

    // [autoload-dev]
    assert_eq!(
        phast_toml["autoload-dev"]["psr-4"]["Acme\\Tests\\"]
            .as_array()
            .unwrap(),
        &vec![toml::Value::String("tests/".into())]
    );

    // [config]
    assert_eq!(
        phast_toml["config"]["minimum-stability"].as_str(),
        Some("dev")
    );
    assert_eq!(phast_toml["config"]["prefer-stable"].as_bool(), Some(true));
}

#[test]
fn migrate_produces_phast_toml() {
    let project = project(COMPOSER_JSON);

    project.cmd().arg("migrate").assert().success();

    assert!(project.root().join("phast.toml").exists());
}

#[test]
fn migrate_fails_without_composer_json() {
    let project = Project::new();

    project.cmd().arg("migrate").assert().failure();
}

#[test]
fn migrate_fails_when_phast_toml_exists() {
    let project = project(COMPOSER_JSON).write("phast.toml", "");

    project.cmd().arg("migrate").assert().failure();
}

#[test]
fn migrate_force_overwrites_phast_toml() {
    let project = project(COMPOSER_JSON).write("phast.toml", "old content");

    project
        .cmd()
        .args(["migrate", "--force"])
        .assert()
        .success();

    assert_ne!(project.read("phast.toml"), "old content");
}

/// Run `migrate` over `composer_json` and return the manifest it produced.
fn migrate(composer_json: &str) -> toml::Value {
    let project = project(composer_json);

    project.cmd().arg("migrate").assert().success();
    project.to_toml()
}
