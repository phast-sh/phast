mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::project::Project;

use indoc::indoc;
use predicates::str::contains;

/// The manifest `init` scaffolded into `dir`, which is a project of its own.
#[rustfmt::skip]
fn scaffolded(project: &Project, dir: &str) -> toml::Value {
    let manifest = project
        .read(&format!("{dir}/phast.toml"));
    toml::from_str(&manifest)
        .expect("scaffolded phast.toml is not valid TOML")
}

/// `--php` is passed almost everywhere so the tests do not depend on a PHP runtime
/// being installed. Detection itself is covered by its own tests.
const PHP_CONSTRAINT: &str = "^8.4";

/// A stand-in for `php` that answers `--version` like PHP 8.9, a release that does not exist,
/// so a manifest asking for `^8.9` could only have come from here. Swallows any script sent
/// on stdin.
const PHP_BIN: &str = indoc! {r#"
    #!/bin/sh
    if [ "$1" = "--version" ]; then
        echo "PHP 8.9.0 (cli) (built: Jan  1 2026 00:00:00) (NTS)"
    else
        cat > /dev/null
    fi
"#};

#[test]
fn init_writes_a_manifest_without_prompting() {
    let project = Project::new();

    project
        .cmd()
        .args([
            "init",
            "--yes",
            "--name",
            "acme/blog",
            "--php",
            PHP_CONSTRAINT,
        ])
        .assert()
        .success();

    let toml = project.to_toml();
    assert_eq!(toml["package"]["name"].as_str(), Some("acme/blog"));
    assert_eq!(toml["package"]["type"].as_str(), Some("library"));
    assert_eq!(toml["require"]["php"].as_str(), Some(PHP_CONSTRAINT));
}

/// The test harness gives the child no terminal, so this exercises the same path a
/// CI job or a pipe takes. Without the fallback it would block on a read forever.
#[test]
fn init_needs_no_yes_flag_when_there_is_no_terminal() {
    let project = Project::new();

    project
        .cmd()
        .args(["init", "--name", "acme/blog", "--php", PHP_CONSTRAINT])
        .assert()
        .success();

    assert_eq!(
        project.to_toml()["package"]["name"].as_str(),
        Some("acme/blog")
    );
}

#[test]
fn init_scaffolds_into_a_given_path() {
    let project = Project::new();
    let target = project.root().join("my-package");

    project
        .cmd()
        .args(["init", "--yes", "--php", PHP_CONSTRAINT])
        .arg(&target)
        .assert()
        .success();

    let name = scaffolded(&project, "my-package")["package"]["name"]
        .as_str()
        .expect("name is not a string")
        .to_string();

    assert!(
        name.ends_with("/my-package"),
        "expected the directory name as the package, got '{name}'"
    );
}

#[test]
fn init_creates_src_and_tests_by_default() {
    let project = Project::new();

    project
        .cmd()
        .args([
            "init",
            "--yes",
            "--name",
            "acme/blog",
            "--php",
            PHP_CONSTRAINT,
        ])
        .assert()
        .success();

    assert!(project.root().join("src").is_dir());
    assert!(project.root().join("tests").is_dir());

    let toml = project.to_toml();
    assert_eq!(
        toml["autoload"]["psr-4"]["Acme\\Blog\\"][0].as_str(),
        Some("src/")
    );
    assert_eq!(
        toml["autoload-dev"]["psr-4"]["Tests\\"][0].as_str(),
        Some("tests/")
    );
}

#[test]
fn init_skips_the_directories_when_told_to() {
    let project = Project::new();

    project
        .cmd()
        .args([
            "init",
            "--yes",
            "--name",
            "acme/blog",
            "--php",
            PHP_CONSTRAINT,
        ])
        .args(["--no-src", "--no-tests"])
        .assert()
        .success();

    assert!(!project.root().join("src").exists());
    assert!(!project.root().join("tests").exists());

    let toml = project.to_toml();
    assert!(toml.get("autoload").is_none());
    assert!(toml.get("autoload-dev").is_none());
}

#[test]
fn init_accepts_a_package_type() {
    let project = Project::new();

    project
        .cmd()
        .args([
            "init",
            "--yes",
            "--name",
            "acme/blog",
            "--php",
            PHP_CONSTRAINT,
        ])
        .args(["--type", "project"])
        .assert()
        .success();

    assert_eq!(
        project.to_toml()["package"]["type"].as_str(),
        Some("project")
    );
}

#[test]
fn init_records_the_php_constraint_in_require_and_platform() {
    let project = Project::new();

    project
        .cmd()
        .args(["init", "--yes", "--name", "acme/blog", "--php", "^8.1"])
        .assert()
        .success();

    let toml = project.to_toml();
    assert_eq!(toml["require"]["php"].as_str(), Some("^8.1"));
    assert_eq!(toml["config"]["platform"]["php"].as_str(), Some("8.1"));
}

#[test]
fn init_lowercases_the_package_name() {
    let project = Project::new();

    project
        .cmd()
        .args([
            "init",
            "--yes",
            "--name",
            "Acme/My-Blog",
            "--php",
            PHP_CONSTRAINT,
        ])
        .assert()
        .success();

    let toml = project.to_toml();
    assert_eq!(toml["package"]["name"].as_str(), Some("acme/my-blog"));
    assert!(toml["autoload"]["psr-4"].get("Acme\\MyBlog\\").is_some());
}

#[test]
fn init_records_a_description_only_when_given() {
    let project = Project::new();
    let with = project.root().join("with");
    let without = project.root().join("without");

    project
        .cmd()
        .args([
            "init",
            "--yes",
            "--name",
            "acme/blog",
            "--php",
            PHP_CONSTRAINT,
        ])
        .args(["--description", "A blog"])
        .arg(&with)
        .assert()
        .success();

    project
        .cmd()
        .args([
            "init",
            "--yes",
            "--name",
            "acme/blog",
            "--php",
            PHP_CONSTRAINT,
        ])
        .arg(&without)
        .assert()
        .success();

    assert_eq!(
        scaffolded(&project, "with")["package"]["description"].as_str(),
        Some("A blog")
    );
    assert!(
        scaffolded(&project, "without")["package"]
            .get("description")
            .is_none()
    );
}

#[test]
fn init_rejects_an_unknown_package_type() {
    let project = Project::new();

    project
        .cmd()
        .args(["init", "--yes", "--type", "nonsense"])
        .assert()
        .failure()
        .stderr(contains("unknown package type: 'nonsense'"));

    assert!(!project.root().join("phast.toml").exists());
}

#[test]
fn init_rejects_an_invalid_package_name() {
    let project = Project::new();

    project
        .cmd()
        .args(["init", "--yes", "--name", "mypackage"])
        .assert()
        .failure()
        .stderr(contains("invalid package name: 'mypackage'"));

    assert!(!project.root().join("phast.toml").exists());
}

#[test]
fn init_refuses_to_overwrite_an_existing_manifest() {
    let project = Project::new();

    project
        .cmd()
        .args([
            "init",
            "--yes",
            "--name",
            "acme/blog",
            "--php",
            PHP_CONSTRAINT,
        ])
        .assert()
        .success();

    project
        .cmd()
        .args([
            "init",
            "--yes",
            "--name",
            "other/name",
            "--php",
            PHP_CONSTRAINT,
        ])
        .assert()
        .failure();

    assert_eq!(
        project.to_toml()["package"]["name"].as_str(),
        Some("acme/blog"),
        "the original manifest must survive the refused run"
    );
}

/// With no runtime to detect and no `--php`, there is nothing left to ask, so the
/// run fails rather than guessing a constraint the resolver would reject later.
#[test]
fn init_fails_without_a_detectable_php_version() {
    let project = Project::new();

    project
        .cmd()
        .args(["init", "--yes", "--name", "acme/blog"])
        .env("PATH", "")
        .assert()
        .failure()
        .stderr(contains("could not detect a PHP version"));

    assert!(!project.root().join("phast.toml").exists());
}

#[test]
fn init_detects_php_through_php_bin() {
    let project = Project::new().write("php-bin", PHP_BIN);
    let php_bin = project.root().join("php-bin");
    fs::set_permissions(&php_bin, fs::Permissions::from_mode(0o755))
        .expect("failed to make the stand-in PHP executable");

    project
        .cmd()
        .env("PHAST_PHP_BIN", &php_bin)
        .args([
            "init",
            "--yes",
            "--name",
            "acme/blog",
            "--no-src",
            "--no-tests",
            "app",
        ])
        .assert()
        .success();

    assert_eq!(
        scaffolded(&project, "app")["require"]["php"].as_str(),
        Some("^8.9"),
        "PHAST_PHP_BIN was not used",
    );
}
