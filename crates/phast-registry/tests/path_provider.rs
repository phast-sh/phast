use std::fs;
use std::path::Path;

use phast_core::lock::phast::{DistType, PackageDef};
use phast_core::package::PackageName;
use phast_registry::provider::{Lockable, PathProvider};
use phast_resolver::provider::Provider;
use phast_version::parser::parse_version;

use indoc::indoc;
use tempfile::TempDir;

/// The minimum a path package must declare: a name and a version.
const PACKAGE_TOML: &str = indoc! {r#"
    [package]
    name    = "acme/pkg"
    version = "1.0.0"
"#};

/// A project directory holding each `(path, contents)` file, with the directories
/// above it created on the way.
fn project(files: &[(&str, &str)]) -> TempDir {
    let tmp = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let file = tmp.path().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, contents).unwrap();
    }
    tmp
}

/// The name `PACKAGE_TOML` declares.
fn name() -> PackageName {
    PackageName::new("acme/pkg")
}

/// The provider for the package at `pkg/`, with no version pinned by the source.
fn provider(tmp: &TempDir) -> PathProvider {
    PathProvider::load(tmp.path(), "pkg", None).unwrap()
}

/// Why `load` refused the package at `rel`, or a panic if it did not.
fn refusal(base: &Path, rel: &str, declared_version: Option<&str>) -> String {
    match PathProvider::load(base, rel, declared_version) {
        Ok(_) => panic!("expected the package at {rel} to be refused"),
        Err(error) => format!("{error:#}"),
    }
}

/// The lock entry `provider` builds for the one version it offers.
async fn locked(provider: &PathProvider) -> PackageDef {
    let name = provider.name().clone();
    let versions = provider.versions(&name).await.unwrap();
    provider.lock(&name, &versions[0]).await.unwrap()
}

// ---------------------------------------------------------------------------
// Which manifest is read
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_phast_toml_package_is_read_from_disk() {
    let manifest = indoc! {r#"
        [package]
        name    = "acme/local-lib"
        version = "1.2.3"
        type    = "library"

        [require]
        "psr/log" = "^3.0"

        [autoload.psr-4]
        "Acme\\LocalLib\\" = "src/"
    "#};
    let tmp = project(&[("packages/local-lib/phast.toml", manifest)]);

    let provider = PathProvider::load(tmp.path(), "packages/local-lib", None).unwrap();
    let name = PackageName::new("acme/local-lib");
    assert_eq!(provider.name(), &name);

    let versions = provider.versions(&name).await.unwrap();
    assert_eq!(versions, vec![parse_version("1.2.3").unwrap()]);

    let meta = provider.metadata(&name, &versions[0]).await.unwrap();
    assert_eq!(meta.require.len(), 1);
    assert_eq!(meta.require[0].0, PackageName::new("psr/log"));
}

#[tokio::test]
async fn a_composer_json_package_is_read_the_same_way() {
    let manifest = indoc! {r#"
        {
            "name": "acme/widget",
            "version": "2.0.0",
            "require": {
                "psr/log": "^3.0"
            }
        }
    "#};
    let tmp = project(&[("vendor-src/widget/composer.json", manifest)]);

    let provider = PathProvider::load(tmp.path(), "vendor-src/widget", None).unwrap();
    let name = PackageName::new("acme/widget");
    assert_eq!(
        provider.versions(&name).await.unwrap(),
        vec![parse_version("2.0.0").unwrap()],
    );
}

#[tokio::test]
async fn phast_toml_is_preferred_over_composer_json() {
    // The versions differ so the assertion below can only pass for one of them.
    let phast_toml = indoc! {r#"
        [package]
        name    = "acme/pkg"
        version = "9.9.9"
    "#};
    let composer_json = indoc! {r#"
        {
            "name": "acme/pkg",
            "version": "1.0.0"
        }
    "#};
    let tmp = project(&[
        ("pkg/phast.toml", phast_toml),
        ("pkg/composer.json", composer_json),
    ]);

    let locked = locked(&provider(&tmp)).await;
    assert_eq!(locked.version, "9.9.9");
}

#[tokio::test]
async fn a_directory_with_no_manifest_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("empty")).unwrap();

    // The shared loader reports the missing manifest and the path context wraps
    // it, so the message names the repository the user wrote, not just the dir.
    let error = refusal(tmp.path(), "empty", None);
    assert!(error.contains("path repository 'empty'"), "{error}");
    assert!(
        error.contains("neither phast.toml nor composer.json"),
        "{error}",
    );
}

#[tokio::test]
async fn a_package_declaring_no_name_is_refused() {
    let manifest = indoc! {r#"
        [package]
        version = "1.0.0"
    "#};
    let tmp = project(&[("pkg/phast.toml", manifest)]);

    // A path package is routed by the name it declares, so there is nothing to
    // pin it under.
    let error = refusal(tmp.path(), "pkg", None);
    assert!(error.contains("declares no package name"), "{error}");
}

// ---------------------------------------------------------------------------
// Which version the package is pinned at
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_version_pinned_by_the_source_wins_over_the_manifest() {
    let tmp = project(&[("pkg/phast.toml", PACKAGE_TOML)]);

    // The manifest says 1.0.0 and the [sources] entry says 3.4.5. The pin wins,
    // so a project can hold a local checkout at a version it does not declare.
    let provider = PathProvider::load(tmp.path(), "pkg", Some("3.4.5")).unwrap();
    let versions = provider.versions(&name()).await.unwrap();
    assert_eq!(versions, vec![parse_version("3.4.5").unwrap()]);
    assert_eq!(locked(&provider).await.version, "3.4.5");
}

#[tokio::test]
async fn a_pinned_version_carries_a_package_that_declares_none() {
    let manifest = indoc! {r#"
        [package]
        name = "acme/pkg"
    "#};
    let tmp = project(&[("pkg/phast.toml", manifest)]);

    let provider = PathProvider::load(tmp.path(), "pkg", Some("2.0.0")).unwrap();
    assert_eq!(
        provider.versions(&name()).await.unwrap(),
        vec![parse_version("2.0.0").unwrap()],
    );
}

#[tokio::test]
async fn a_branch_may_be_pinned_as_the_version() {
    let tmp = project(&[("pkg/phast.toml", PACKAGE_TOML)]);

    let provider = PathProvider::load(tmp.path(), "pkg", Some("dev-main")).unwrap();
    assert_eq!(locked(&provider).await.version, "dev-main");
}

#[tokio::test]
async fn a_pinned_version_that_does_not_parse_is_refused() {
    let tmp = project(&[("pkg/phast.toml", PACKAGE_TOML)]);

    let error = refusal(tmp.path(), "pkg", Some("not a version"));
    assert!(error.contains("invalid version"), "{error}");
    assert!(error.contains("acme/pkg"), "{error}");
}

#[tokio::test]
async fn a_package_with_no_version_falls_back_to_the_checkout_branch() {
    let manifest = indoc! {r#"
        [package]
        name = "acme/pkg"
    "#};
    let tmp = project(&[
        ("pkg/phast.toml", manifest),
        (".git/HEAD", "ref: refs/heads/trunk\n"),
    ]);

    // How a version-less monorepo component is pinned at all: the branch of the
    // checkout that contains it becomes its version.
    let provider = provider(&tmp);
    assert_eq!(
        provider.versions(&name()).await.unwrap(),
        vec![parse_version("dev-trunk").unwrap()],
    );
}

#[tokio::test]
async fn a_package_with_no_version_outside_a_checkout_is_refused() {
    let manifest = indoc! {r#"
        [package]
        name = "acme/pkg"
    "#};
    let tmp = project(&[("pkg/phast.toml", manifest)]);

    // No pin, no declared version and no branch to guess from leaves nothing to
    // resolve against, and a package cannot be locked unversioned.
    let error = refusal(tmp.path(), "pkg", None);
    assert!(error.contains("declares no version"), "{error}");
}

// ---------------------------------------------------------------------------
// What the resolver is told
// ---------------------------------------------------------------------------

#[tokio::test]
async fn replace_and_provide_specs_resolve_against_the_package_version() {
    let manifest = indoc! {r#"
        [package]
        name    = "acme/bundle"
        version = "1.2.3"

        [replace]
        "acme/old" = "self.version"

        [provide]
        "virtual/iface" = "2.0.0"
    "#};
    let tmp = project(&[("pkg/phast.toml", manifest)]);

    let provider = provider(&tmp);
    let name = PackageName::new("acme/bundle");
    let version = parse_version("1.2.3").unwrap();
    let meta = provider.metadata(&name, &version).await.unwrap();

    // `self.version` is the package's own version, and an exact spec is itself,
    // so a monorepo sibling pin lands the way a registry package's would.
    let replaced = parse_version("1.2.3").unwrap();
    assert_eq!(meta.replace, vec![(PackageName::new("acme/old"), replaced)]);
    let provided = parse_version("2.0.0").unwrap();
    assert_eq!(
        meta.provide,
        vec![(PackageName::new("virtual/iface"), provided)],
    );
}

#[tokio::test]
async fn dev_requirements_are_never_offered() {
    let manifest = indoc! {r#"
        [package]
        name    = "acme/pkg"
        version = "1.0.0"

        [require-dev]
        "phpunit/phpunit" = "^10.0"
    "#};
    let tmp = project(&[("pkg/phast.toml", manifest)]);

    // An installed dependency's own dev requirements are not resolved, whoever
    // supplied it, so a local checkout does not drag its test tooling in.
    let meta = provider(&tmp)
        .metadata(&name(), &parse_version("1.0.0").unwrap())
        .await
        .unwrap();
    assert!(meta.require_dev.is_empty());
}

#[tokio::test]
async fn versions_are_empty_for_a_name_the_provider_does_not_serve() {
    let tmp = project(&[("pkg/phast.toml", PACKAGE_TOML)]);
    let other = PackageName::new("someone/else");

    assert!(provider(&tmp).versions(&other).await.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// The lock entry
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_lock_entry_is_a_path_dist_rather_than_a_member() {
    let tmp = project(&[("pkg/phast.toml", PACKAGE_TOML)]);
    let locked = locked(&provider(&tmp)).await;

    assert_eq!(locked.name, "acme/pkg");
    assert!(locked.path.is_none(), "path repo is not a workspace member");

    // The package is materialized by symlink rather than downloaded, so there is
    // nothing to verify and nowhere to fetch from.
    let dist = locked.dist.expect("path package must carry a dist");
    assert_eq!(dist.dist_type, DistType::Path);
    assert_eq!(dist.url, "pkg");
    assert!(dist.reference.is_none());
    assert!(dist.shasum.is_none());
    assert!(locked.source.is_none());
}

#[tokio::test]
async fn the_source_directory_is_recorded_as_the_source_declared_it() {
    let tmp = project(&[("packages/nested/deep/phast.toml", PACKAGE_TOML)]);

    // `dist.url` is read back relative to the project root when the symlink is
    // laid down, so it is stored exactly as written rather than canonicalized.
    let provider = PathProvider::load(tmp.path(), "packages/nested/deep", None).unwrap();
    let dist = locked(&provider).await.dist.unwrap();
    assert_eq!(dist.url, "packages/nested/deep");
}

#[tokio::test]
async fn the_manifest_fields_a_consumer_reads_are_carried_over() {
    let manifest = indoc! {r#"
        [package]
        name        = "acme/pkg"
        version     = "1.0.0"
        type        = "library"
        description = "A local package"
        license     = "MIT"

        [require]
        "psr/log" = "^3.0"

        [autoload.psr-4]
        "Acme\\Pkg\\" = "src/"

        [extra]
        branch-alias = { dev-main = "1.x-dev" }
    "#};
    let tmp = project(&[("pkg/phast.toml", manifest)]);
    let locked = locked(&provider(&tmp)).await;

    assert_eq!(locked.package_type.as_deref(), Some("library"));
    assert_eq!(locked.description.as_deref(), Some("A local package"));
    assert_eq!(locked.license, ["MIT"]);
    assert_eq!(
        locked.require.entries.get("psr/log").map(String::as_str),
        Some("^3.0"),
    );
    assert!(locked.autoload.psr4.contains_key("Acme\\Pkg\\"));

    let extra = locked.extra.expect("extra is carried into the lock");
    assert_eq!(extra.data["branch-alias"]["dev-main"], "1.x-dev");
}

#[tokio::test]
async fn a_version_string_is_normalized_the_way_a_registry_package_is() {
    let manifest = indoc! {r#"
        {
            "name": "acme/pkg",
            "version": "2.0.0"
        }
    "#};
    let tmp = project(&[("pkg/composer.json", manifest)]);

    // Parsed and rendered back rather than copied, which trims trailing zeros,
    // so a path package and a registry package name the same release alike.
    assert_eq!(locked(&provider(&tmp)).await.version, "2.0");
}

#[tokio::test]
async fn dev_requirements_are_not_written_to_the_lock() {
    let manifest = indoc! {r#"
        [package]
        name    = "acme/pkg"
        version = "1.0.0"

        [require-dev]
        "phpunit/phpunit" = "^10.0"
    "#};
    let tmp = project(&[("pkg/phast.toml", manifest)]);

    // The entry describes an installed dependency, and nothing installs another
    // package's dev requirements.
    assert!(locked(&provider(&tmp)).await.require_dev.entries.is_empty());
}

#[tokio::test]
async fn locking_another_name_is_an_error() {
    let tmp = project(&[("pkg/phast.toml", PACKAGE_TOML)]);
    let other = PackageName::new("someone/else");
    let version = parse_version("1.0.0").unwrap();

    // `versions` answers an unserved name with an empty list, but `lock` must not
    // hand back this package's entry under a name that was never asked for.
    let error = match provider(&tmp).lock(&other, &version).await {
        Ok(_) => panic!("locking an unserved package must fail"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("acme/pkg"), "got: {error}");
    assert!(error.contains("someone/else"), "got: {error}");
}
