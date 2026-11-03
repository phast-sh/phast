use phast_core::manifest::Manifest;
use phast_core::manifest::PlatformCheck;
use phast_core::manifest::Repository;
use phast_core::package::PackageName;
use phast_version::parser::{parse_constraint, parse_version};
use phast_version::{Constraint, RequireSpec, Stability, VersionSpec};

use std::time::Duration;

use indoc::indoc;
use tempfile::TempDir;

/// A package name, for a string a test knows is valid.
fn package(name: &str) -> PackageName {
    PackageName::new(name)
}

/// A parsed constraint, for a string a test knows is valid.
fn constraint(s: &str) -> Constraint {
    parse_constraint(s).unwrap()
}

/// A `require` / `require-dev` / `conflict` value wrapping a concrete constraint.
fn require(s: &str) -> RequireSpec {
    RequireSpec::Constraint {
        constraint: constraint(s),
        stability: None,
    }
}

// ---------------------------------------------------------------------------
// composer.json parsing
// ---------------------------------------------------------------------------

#[test]
fn parses_minimal_composer_json() {
    let json = indoc! {r#"
        {
            "require": {
                "vendor/foo": "^1.0"
            }
        }
    "#};
    let manifest = Manifest::from_composer_json(json).unwrap();

    assert_eq!(manifest.require[&package("vendor/foo")], require("^1.0"));
    assert!(manifest.require_dev.is_empty());
}

#[test]
fn parses_full_composer_json() {
    let json = indoc! {r#"
        {
            "name": "namespace/app",
            "description": "A test app",
            "type": "project",
            "license": "MIT",
            "require": {
                "php": "^8.1",
                "vendor/a": "^2.0",
                "vendor/b": ">=1.5 <3.0"
            },
            "require-dev": {
                "vendor/test": "^1.0"
            },
            "minimum-stability": "beta",
            "prefer-stable": true,
            "config": {
                "platform": {
                    "php": "8.2.0"
                }
            },
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                },
                "files": ["bootstrap.php"]
            },
            "autoload-dev": {
                "psr-4": {
                    "Tests\\": ["tests/", "integration/"]
                }
            }
        }
    "#};

    let manifest = Manifest::from_composer_json(json).unwrap();

    assert_eq!(manifest.name.as_deref(), Some("namespace/app"));
    assert_eq!(manifest.description.as_deref(), Some("A test app"));
    assert_eq!(manifest.package_type.as_deref(), Some("project"));
    assert_eq!(manifest.license.as_deref(), Some("MIT"));

    assert_eq!(manifest.require[&package("php")], require("^8.1"));
    assert_eq!(manifest.require[&package("vendor/a")], require("^2.0"));
    assert_eq!(
        manifest.require[&package("vendor/b")],
        require(">=1.5 <3.0")
    );
    assert_eq!(
        manifest.require_dev[&package("vendor/test")],
        require("^1.0")
    );

    assert_eq!(manifest.config.minimum_stability, Stability::Beta);
    assert!(manifest.config.prefer_stable);
    assert_eq!(manifest.config.platform["php"], "8.2.0");

    assert_eq!(manifest.autoload.psr4["App\\"], vec!["src/".to_string(),]);
    assert_eq!(manifest.autoload.files, vec!["bootstrap.php",]);
    assert_eq!(
        manifest.autoload_dev.psr4["Tests\\"],
        vec!["tests/".to_string(), "integration/".to_string(),]
    );
}

#[test]
fn composer_json_names_normalized_to_lowercase() {
    let json = indoc! {r#"
        {
            "require": {
                "Vendor/Foo": "^1.0"
            }
        }
    "#};
    let manifest = Manifest::from_composer_json(json).unwrap();

    assert!(manifest.require.contains_key(&package("vendor/foo")));
}

#[test]
fn composer_json_default_stability_is_stable() {
    let json = indoc! {r#"
        {
            "require": {}
        }
    "#};
    let manifest = Manifest::from_composer_json(json).unwrap();

    assert_eq!(manifest.config.minimum_stability, Stability::Stable);
}

#[test]
fn composer_json_rejects_invalid_stability() {
    let json = indoc! {r#"
        {
            "minimum-stability": "bogus"
        }
    "#};
    let err = Manifest::from_composer_json(json).unwrap_err();

    assert!(matches!(
        err,
        phast_core::manifest::composer::ComposerManifestError::InvalidConfig { .. }
    ));
}

#[test]
fn composer_json_rejects_invalid_constraint() {
    let json = indoc! {r#"
        {
            "require": {
                "vendor/a": ""
            }
        }
    "#};
    let err = Manifest::from_composer_json(json).unwrap_err();

    assert!(matches!(
        err,
        phast_core::manifest::composer::ComposerManifestError::InvalidConstraint { .. }
    ));
}

#[test]
fn composer_json_parses_composer_repository() {
    let json = indoc! {r#"
        {
            "repositories": [
                {
                    "type": "composer",
                    "url": "https://example.com"
                }
            ]
        }
    "#};
    let manifest = Manifest::from_composer_json(json).unwrap();

    assert!(matches!(
        &manifest.repositories[0],
        Repository::Composer { url, .. } if url == "https://example.com"
    ));
}

#[test]
fn composer_json_parses_vcs_repository() {
    let json = indoc! {r#"
        {
            "repositories": [
                {
                    "type": "git",
                    "url": "https://github.com/username/private.git"
                }
            ]
        }
    "#};
    let manifest = Manifest::from_composer_json(json).unwrap();

    assert!(matches!(
        &manifest.repositories[0],
        Repository::Vcs { url, .. } if url == "https://github.com/username/private.git"
    ));
}

#[test]
fn composer_json_skips_unknown_repository_type() {
    let json = indoc! {r#"
        {
            "repositories": [
                {
                    "type": "unknown-type",
                    "url": "https://example.com"
                }
            ]
        }
    "#};
    let manifest = Manifest::from_composer_json(json).unwrap();

    assert!(manifest.repositories.is_empty());
}

// ---------------------------------------------------------------------------
// phast.toml parsing
// ---------------------------------------------------------------------------

#[test]
fn parses_minimal_phast_toml() {
    let toml = indoc! {r#"
        [require]
        "vendor/foo" = "^1.0"
    "#};
    let manifest = Manifest::from_phast_toml(toml).unwrap();

    assert_eq!(manifest.require[&package("vendor/foo")], require("^1.0"));
}

#[test]
fn parses_full_phast_toml() {
    let toml = indoc! {r#"
        [package]
        name        = "namespace/app"
        description = "A test app"
        license     = "MIT"

        [require]
        "php"      = "^8.1"
        "vendor/a" = "^2.0"

        [require-dev]
        "vendor/test" = "^1.0"

        [config]
        minimum-stability      = "beta"
        prefer-stable          = true
        classmap-authoritative = true
        platform-check         = "all"

        [config.platform]
        php = "8.2.0"

        [autoload.psr-4]
        "App\\" = "src/"

        [autoload-dev.psr-4]
        "Tests\\" = ["tests/", "integration/"]
    "#};

    let manifest = Manifest::from_phast_toml(toml).unwrap();

    assert_eq!(manifest.name.as_deref(), Some("namespace/app"));
    assert_eq!(manifest.require[&package("php")], require("^8.1"));
    assert_eq!(manifest.require[&package("vendor/a")], require("^2.0"));
    assert_eq!(
        manifest.require_dev[&package("vendor/test")],
        require("^1.0")
    );
    assert_eq!(manifest.config.minimum_stability, Stability::Beta);
    assert!(manifest.config.prefer_stable);
    assert!(manifest.config.classmap_authoritative);
    assert_eq!(manifest.config.platform["php"], "8.2.0");
    assert_eq!(manifest.config.platform_check, PlatformCheck::All);
    assert_eq!(manifest.autoload.psr4["App\\"], vec!["src/"]);
    assert_eq!(
        manifest.autoload_dev.psr4["Tests\\"],
        vec!["tests/".to_string(), "integration/".to_string()]
    );
}

#[test]
fn phast_toml_names_normalized_to_lowercase() {
    let toml = indoc! {r#"
        [require]
        "Vendor/Foo" = "^1.0"
    "#};
    let manifest = Manifest::from_phast_toml(toml).unwrap();

    assert!(manifest.require.contains_key(&package("vendor/foo")));
}

#[test]
fn phast_toml_default_stability_is_stable() {
    let toml = "";
    let manifest = Manifest::from_phast_toml(toml).unwrap();

    assert_eq!(manifest.config.minimum_stability, Stability::Stable);
}

#[test]
fn phast_toml_scripts_take_a_string_or_a_list() {
    let toml = indoc! {r#"
        [scripts]
        lint = "phpcs"
        test = ["phpunit", "phpstan analyse"]
    "#};
    let manifest = Manifest::from_phast_toml(toml).unwrap();

    // Both shapes are kept as written rather than normalized to one, so a manifest
    // round-trips looking the way its author left it.
    assert_eq!(manifest.scripts["lint"].as_str(), vec!["phpcs"]);
    assert_eq!(
        manifest.scripts["test"].as_str(),
        vec!["phpunit", "phpstan analyse"],
    );
}

#[test]
fn phast_toml_carries_extra_through_as_json() {
    let toml = indoc! {r#"
        [extra]
        laravel = { providers = ["Acme\\Provider"] }
    "#};
    let manifest = Manifest::from_phast_toml(toml).unwrap();

    // Phast reads nothing out of `extra`; it is carried so tooling that does,
    // like Laravel's package discovery, still finds what it put there.
    assert_eq!(
        manifest.extra,
        serde_json::json!({"laravel": {"providers": ["Acme\\Provider"]}}),
    );
}

#[test]
fn phast_toml_minimum_package_age_needs_a_unit() {
    let toml = indoc! {r#"
        [config]
        minimum-package-age = "7d"
    "#};
    let manifest = Manifest::from_phast_toml(toml).unwrap();
    assert_eq!(
        manifest.config.minimum_package_age,
        Some(Duration::from_secs(7 * 24 * 60 * 60)),
    );

    // A bare number is refused rather than guessed at, and the message says which
    // units there are to choose from.
    let err = Manifest::from_phast_toml(indoc! {r#"
        [config]
        minimum-package-age = "604800"
    "#})
    .unwrap_err()
    .to_string();
    assert!(err.contains("expected h, d, or w"), "{err}");
}

#[test]
fn phast_toml_secure_http_can_be_turned_off() {
    assert!(Manifest::from_phast_toml("").unwrap().config.secure_http);

    let toml = indoc! {r#"
        [config]
        secure-http = false
    "#};
    assert!(!Manifest::from_phast_toml(toml).unwrap().config.secure_http);
}

// ---------------------------------------------------------------------------
// Shared: both formats produce the same Manifest for equivalent input
// ---------------------------------------------------------------------------

#[test]
fn composer_and_phast_produce_equivalent_manifests() {
    let composer_json = indoc! {r#"
        {
            "name": "namespace/app",
            "require": {
                "vendor/lib": "^1.0"
            },
            "config": {
                "minimum-stability": "stable"
            }
        }
    "#};

    let phast_toml = indoc! {r#"
        [package]
        name = "namespace/app"

        [require]
        "vendor/lib" = "^1.0"

        [config]
        minimum-stability = "stable"
    "#};

    let from_composer = Manifest::from_composer_json(composer_json).unwrap();
    let from_phast = Manifest::from_phast_toml(phast_toml).unwrap();

    assert_eq!(from_composer.name, from_phast.name);
    assert_eq!(from_composer.require, from_phast.require);
    assert_eq!(
        from_composer.config.minimum_stability,
        from_phast.config.minimum_stability
    );
}

// ---------------------------------------------------------------------------
// File discovery: Manifest::load picks the right file from a directory
// ---------------------------------------------------------------------------

#[test]
fn load_errors_when_no_manifest_found() {
    let tmp = TempDir::new().unwrap();
    let err = Manifest::load(tmp.path()).unwrap_err();
    assert!(
        err.to_string()
            .contains("neither phast.toml nor composer.json")
    );
}

#[test]
fn load_names_the_file_that_failed_to_parse() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("phast.toml"), "this is not toml [[[").unwrap();

    let err = Manifest::load(tmp.path()).unwrap_err();
    assert!(
        err.to_string().contains("phast.toml is invalid"),
        "unexpected error: {err:#}",
    );
}

#[test]
fn load_names_composer_json_when_it_fails_to_parse() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("composer.json"), "{ not json").unwrap();

    let err = Manifest::load(tmp.path()).unwrap_err();
    assert!(
        err.to_string().contains("composer.json is invalid"),
        "unexpected error: {err:#}",
    );
}

#[test]
fn load_reads_phast_toml() {
    let tmp = TempDir::new().unwrap();
    let manifest = indoc! {r#"
        [package]
        name = "vendor/pkg"
    "#};

    std::fs::write(tmp.path().join("phast.toml"), manifest).unwrap();

    let manifest = Manifest::load(tmp.path()).unwrap();
    assert_eq!(manifest.name.as_deref(), Some("vendor/pkg"));
}

#[test]
fn load_reads_composer_json() {
    let tmp = TempDir::new().unwrap();
    let manifest = indoc! {r#"
        {
            "name": "vendor/pkg"
        }
    "#};

    std::fs::write(tmp.path().join("composer.json"), manifest).unwrap();

    let manifest = Manifest::load(tmp.path()).unwrap();
    assert_eq!(manifest.name.as_deref(), Some("vendor/pkg"));
}

#[test]
fn load_prefers_phast_toml_over_composer_json() {
    let tmp = TempDir::new().unwrap();
    // Each names the package after the file it came from, so the assertion says which
    // one was read.
    let phast_toml = indoc! {r#"
        [package]
        name = "vendor/from-phast"
    "#};
    let composer_json = indoc! {r#"
        { "name": "vendor/from-composer" }
    "#};

    std::fs::write(tmp.path().join("phast.toml"), phast_toml).unwrap();
    std::fs::write(tmp.path().join("composer.json"), composer_json).unwrap();

    let manifest = Manifest::load(tmp.path()).unwrap();
    assert_eq!(manifest.name.as_deref(), Some("vendor/from-phast"));
}

// ---------------------------------------------------------------------------
// replace / provide are version specs (self.version, *, exact), not constraints
// ---------------------------------------------------------------------------

#[test]
fn composer_json_preserves_replace_specs() {
    // self.version (which is not a constraint) must not error or be dropped.
    let json = indoc! {r#"
        {
            "name": "laravel/framework",
            "replace": {
                "illuminate/auth": "self.version",
                "spatie/once": "*",
                "vendor/pinned": "1.2.3"
            }
        }
    "#};

    let manifest = Manifest::from_composer_json(json).unwrap();

    assert_eq!(
        manifest.replace[&package("illuminate/auth")],
        VersionSpec::SelfVersion
    );
    assert_eq!(manifest.replace[&package("spatie/once")], VersionSpec::Any);
    assert_eq!(
        manifest.replace[&package("vendor/pinned")],
        VersionSpec::Exact(parse_version("1.2.3").unwrap()),
    );
}

// ---------------------------------------------------------------------------
// require / require-dev / conflict may carry self.version (RequireSpec)
// ---------------------------------------------------------------------------

#[test]
fn composer_json_preserves_self_version_in_require() {
    // A monorepo component pins a sibling to the package's own version. self.version
    // must survive parsing as a RequireSpec rather than error as an invalid constraint.
    let json = indoc! {r#"
        {
            "name": "symfony/symfony",
            "require": {
                "symfony/console": "self.version"
            },
            "require-dev": {
                "symfony/phpunit-bridge": "self.version"
            },
            "conflict": {
                "symfony/legacy": "self.version"
            }
        }
    "#};

    let manifest = Manifest::from_composer_json(json).unwrap();

    assert_eq!(
        manifest.require[&package("symfony/console")],
        RequireSpec::SelfVersion
    );
    assert_eq!(
        manifest.require_dev[&package("symfony/phpunit-bridge")],
        RequireSpec::SelfVersion
    );
    assert_eq!(
        manifest.conflict[&package("symfony/legacy")],
        RequireSpec::SelfVersion
    );
}

#[test]
fn phast_toml_preserves_self_version_in_require() {
    let toml = "\
        [package]\nname = \"acme/monorepo\"\n\n\
        [require]\n\"acme/core\" = \"self.version\"\n\"vendor/lib\" = \"^1.0\"\n\n\
        [conflict]\n\"acme/legacy\" = \"self.version\"\n";

    let manifest = Manifest::from_phast_toml(toml).unwrap();

    assert_eq!(
        manifest.require[&package("acme/core")],
        RequireSpec::SelfVersion
    );
    assert_eq!(manifest.require[&package("vendor/lib")], require("^1.0"));
    assert_eq!(
        manifest.conflict[&package("acme/legacy")],
        RequireSpec::SelfVersion
    );
}

#[test]
fn phast_toml_preserves_provide_specs() {
    let toml = "\
        [package]\nname = \"acme/lib\"\n\n\
        [provide]\n\"acme/old\" = \"self.version\"\n\"virtual/iface\" = \"1.0.0\"\n";

    let manifest = Manifest::from_phast_toml(toml).unwrap();

    assert_eq!(
        manifest.provide[&package("acme/old")],
        VersionSpec::SelfVersion
    );
    assert_eq!(
        manifest.provide[&package("virtual/iface")],
        VersionSpec::Exact(parse_version("1.0.0").unwrap()),
    );
}
