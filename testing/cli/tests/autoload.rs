//! The `phast autoload` command surface.
//!
//! `autoload` regenerates the autoloader from `phast.lock`: no resolution, no
//! downloads. The fixtures therefore carry a lock and an already-installed `vendor/`
//! tree, and nothing here reaches the network.
//!
//! Note that the root package's own autoload rules come from the lock, not the
//! manifest. The lock is self-contained, so `autoload` never parses `phast.toml` for
//! them; the manifest is read only for `[scripts]` and `[config]`.

mod common;

use common::project::Project;

use indoc::{formatdoc, indoc};
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

const MANIFEST: &str = indoc! {r#"
    [package]
    name = "acme/blog"

    [require]
    "vendor/alpha" = "^1.0"
"#};

/// A lock with one dependency, plus root rules in both the prod and dev sections.
///
/// Unlike the manifest, a lock's psr-4 values are always arrays.
const LOCK: &str = indoc! {r#"
    version      = 1
    content-hash = "sha256:test"

    [metadata]
    generated-by = "phast 0.1.0"
    generated-at = "2026-01-01T00:00:00+00:00"

    [autoload.psr-4]
    "Acme\\Blog\\" = ["src/"]

    [autoload-dev.psr-4]
    "Acme\\Blog\\Tests\\" = ["tests/"]

    [[packages]]
    name    = "vendor/alpha"
    version = "1.0.0"

    [packages.autoload.psr-4]
    "Vendor\\Alpha\\" = ["src/"]
"#};

/// A project with the dependency already extracted under `vendor/`.
fn project() -> Project {
    Project::new()
        .manifest(MANIFEST)
        .lock(LOCK)
        .write("src/.keep", "")
        .write("vendor/vendor/alpha/src/.keep", "")
}

#[test]
fn autoload_writes_the_loader_and_its_maps() {
    let project = project();

    project.cmd().arg("autoload").assert().success();

    for generated in [
        "vendor/autoload.php",
        "vendor/phast/ClassLoader.php",
        "vendor/phast/autoload_real.php",
        "vendor/phast/autoload_psr4.php",
        "vendor/phast/autoload_classmap.php",
        "vendor/phast/installed.php",
        "vendor/phast/InstalledVersions.php",
        "vendor/phast/platform_check.php",
        "vendor/composer/installed.json",
    ] {
        assert!(
            project.root().join(generated).exists(),
            "{generated} was not generated",
        );
    }
}

#[test]
fn autoload_maps_both_the_root_and_its_dependencies() {
    let project = project();

    project.cmd().arg("autoload").assert().success();

    let psr4 = project.read("vendor/phast/autoload_psr4.php");
    assert!(psr4.contains(r"Acme\\Blog\\"), "root rule missing:\n{psr4}");
    assert!(
        psr4.contains(r"Vendor\\Alpha\\"),
        "dependency rule missing:\n{psr4}",
    );
}

#[test]
fn autoload_includes_dev_rules_by_default() {
    let project = project();

    project.cmd().arg("autoload").assert().success();

    assert!(
        project
            .read("vendor/phast/autoload_psr4.php")
            .contains(r"Acme\\Blog\\Tests\\")
    );
}

#[test]
fn autoload_no_dev_drops_the_dev_rules_only() {
    let project = project();

    project
        .cmd()
        .args(["autoload", "--no-dev"])
        .assert()
        .success();

    let psr4 = project.read("vendor/phast/autoload_psr4.php");
    assert!(
        !psr4.contains(r"Acme\\Blog\\Tests\\"),
        "dev rule survived --no-dev:\n{psr4}"
    );
    assert!(psr4.contains(r"Acme\\Blog\\"), "prod rule was dropped too");
    assert!(
        psr4.contains(r"Vendor\\Alpha\\"),
        "dependency was dropped too",
    );
}

/// The generated loader is told whether it may fall back to the filesystem, so the
/// flag has to reach the emitted PHP rather than only the options struct.
#[test]
fn autoload_defaults_to_a_non_authoritative_classmap() {
    let project = project();

    project.cmd().arg("autoload").assert().success();

    assert!(
        project
            .read("vendor/phast/autoload_real.php")
            .contains("setClassMapAuthoritative(false)")
    );
}

#[test]
fn autoload_a_flag_makes_the_classmap_authoritative() {
    let project = project();

    project.cmd().args(["autoload", "-a"]).assert().success();

    assert!(
        project
            .read("vendor/phast/autoload_real.php")
            .contains("setClassMapAuthoritative(true)")
    );
}

/// `[config] classmap-authoritative` reaches the same switch, so a project can set it
/// once rather than remembering the flag.
#[test]
fn autoload_honours_classmap_authoritative_from_the_manifest() {
    let manifest = formatdoc! {r#"
        {MANIFEST}
        [config]
        classmap-authoritative = true
    "#};
    let project = project().manifest(&manifest);

    project.cmd().arg("autoload").assert().success();

    assert!(
        project
            .read("vendor/phast/autoload_real.php")
            .contains("setClassMapAuthoritative(true)")
    );
}

#[test]
fn autoload_runs_the_dump_hooks() {
    let manifest = formatdoc! {r#"
        {MANIFEST}
        [scripts]
        pre-autoload-dump  = "echo PRE_HOOK"
        post-autoload-dump = "echo POST_HOOK"
    "#};
    let project = project().manifest(&manifest);

    project
        .cmd()
        .arg("autoload")
        .assert()
        .success()
        .stdout(contains("PRE_HOOK"))
        .stdout(contains("POST_HOOK"));
}

#[test]
fn autoload_no_scripts_skips_the_dump_hooks() {
    let manifest = formatdoc! {r#"
        {MANIFEST}
        [scripts]
        pre-autoload-dump  = "echo PRE_HOOK"
        post-autoload-dump = "echo POST_HOOK"
    "#};
    let project = project().manifest(&manifest);

    project
        .cmd()
        .args(["autoload", "--no-scripts"])
        .assert()
        .success()
        .stdout(contains("PRE_HOOK").not())
        .stdout(contains("POST_HOOK").not());
}

#[test]
fn autoload_fails_without_a_lock() {
    let project = Project::new().manifest(MANIFEST);

    project
        .cmd()
        .arg("autoload")
        .assert()
        .failure()
        .stderr(contains("could not read phast.lock"));
}

#[test]
fn autoload_fails_without_a_manifest() {
    let project = Project::new().lock(LOCK);

    project
        .cmd()
        .arg("autoload")
        .assert()
        .failure()
        .stderr(contains("neither phast.toml nor composer.json found"));
}
