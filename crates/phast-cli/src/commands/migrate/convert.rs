//! The transforms behind `phast migrate`: converting a parsed `composer.json`
//! into a `PhastManifest` and an `AuthConfig`, and detecting composer.json
//! fields Phast does not yet support.
//!
//! `build_phast_manifest` and `build_auth_manifest` are rooted at the project
//! directory (`base`) because migration reads sibling files: `path` repositories'
//! local manifests for their package names, and existing auth files for credentials.
//!
//! The `migrate` command (`commands::migrate`) is a thin coordinator over these:
//! it does the top-level file I/O and user messaging, these do the conversion.

use std::path::Path;

use phast_core::manifest::Manifest;
use phast_core::manifest::composer::ComposerManifest;
use phast_core::manifest::phast::{
    AutoloadSection, ConfigSection, ConflictSection, DependencySpec, ExtraSection, PackageSection,
    PhastManifest, Platform, ProvideSection, ReplaceSection, RepositoryDef, RequireSection,
    ScriptsSection, SourceSpec, SourcesSection, SuggestSection,
};
use phast_core::toml::from_json;
use phast_registry::http::auth::AuthConfig;

use anyhow::Result;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use serde_json::from_str;
use toml::Value as TomlValue;

/// Top-level `composer.json` keys Phast models: carried into `phast.toml` or used
/// during resolution and install.
#[rustfmt::skip]
const SUPPORTED_FIELDS: &[&str] = &[
    "name",
    "description",
    "type",
    "license",
    "keywords",
    "homepage",
    "authors",
    "support",
    "funding",
    "require",
    "require-dev",
    "conflict",
    "replace",
    "provide",
    "suggest",
    "autoload",
    "autoload-dev",
    "minimum-stability",
    "prefer-stable",
    "scripts",
    "repositories",
    "extra",
    "config",
];

/// Keys Phast deliberately drops; not worth a warning.
///
/// `version` is dropped rather than carried: a hardcoded version in a manifest is
/// derived from the VCS tag in practice, and `[package] version` is left for the
/// author to set deliberately if they want one.
#[rustfmt::skip]
const IGNORED_FIELDS: &[&str] = &[
    "version",
    "time",
    "readme",
    "scripts-descriptions",
    "scripts-aliases",
];

/// Top-level `composer.json` keys Phast neither models nor deliberately ignores,
/// so `migrate` could not carry them over. Sorted, for a stable message.
#[rustfmt::skip]
pub fn detect_unsupported_fields(json: &str) -> Vec<String> {
    let Ok(JsonValue::Object(map)) = from_str::<JsonValue>(json) else {
        return Vec::new();
    };

    let mut fields: Vec<String> = map
        .keys()
        .filter(|k| {
            !SUPPORTED_FIELDS.contains(&k.as_str()) &&
            !IGNORED_FIELDS.contains(&k.as_str())
        })
        .cloned()
        .collect();

    fields.sort();
    fields
}

/// Convert a `composer.json` into a `PhastManifest`, rooted at project directory
/// `base` so `path` repositories can be read for the package names that key their
/// `[sources]` entries.
#[rustfmt::skip]
pub fn build_phast_manifest(base: &Path, manifest: &ComposerManifest) -> PhastManifest {
    let package = PackageSection {
        name: manifest.name.clone(),
        version: None,
        description: manifest.description.clone().filter(|d| !d.is_empty()),
        package_type: manifest.package_type.clone(),
        license: manifest.license.clone(),
        homepage: manifest.homepage.clone(),
        keywords: manifest.keywords.clone(),
        authors: manifest
            .authors
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect(),
        support: manifest
            .support
            .clone()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
        funding: manifest
            .funding
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect(),
    };

    let require = RequireSection {
        dependencies: manifest
            .require
            .iter()
            .map(|(p, v)| (p.to_string(), DependencySpec::Version(v.to_string())))
            .collect(),
    };
    let require_dev = RequireSection {
        dependencies: manifest
            .require_dev
            .iter()
            .map(|(p, v)| (p.to_string(), DependencySpec::Version(v.to_string())))
            .collect(),
    };

    let autoload = AutoloadSection {
        psr4: manifest.autoload
            .psr4
            .iter()
            .map(|(k, v)| (k.clone(), from_json(v)))
            .collect(),
        psr0: manifest.autoload
            .psr0
            .iter()
            .map(|(k, v)| (k.clone(), from_json(v)))
            .collect(),
        classmap: manifest.autoload.classmap.clone(),
        files: manifest.autoload.files.clone(),
        exclude: manifest.autoload.exclude_from_classmap.clone(),
    };

    let autoload_dev = AutoloadSection {
        psr4: manifest.autoload_dev
            .psr4
            .iter()
            .map(|(k, v)| (k.clone(), from_json(v)))
            .collect(),
        psr0: manifest.autoload_dev
            .psr0
            .iter()
            .map(|(k, v)| (k.clone(), from_json(v)))
            .collect(),
        classmap: manifest.autoload_dev.classmap.clone(),
        files: manifest.autoload_dev.files.clone(),
        exclude: manifest.autoload_dev.exclude_from_classmap.clone(),
    };

    // Only copy an explicit config.platform.php,
    // do not derive it from [require].
    let php_platform = manifest
        .config
        .platform
        .get("php")
        .cloned();

    let minimum_stability = manifest
        .minimum_stability
        .as_deref()
        .and_then(|s| serde_json::from_value(JsonValue::String(s.to_owned()))
        .ok());

    let config = ConfigSection {
        minimum_stability,
        // TODO: should we establish `min age` guardrail by default?
        minimum_package_age: None,
        prefer_stable: manifest.prefer_stable,
        classmap_authoritative: false,
        secure_http: manifest.config.secure_http,
        sort_packages: manifest.config.sort_packages,
        platform: Platform {
            php: php_platform,
            extensions: IndexMap::new(),
        },
        platform_check: None,
        // TODO: decide whether Phast supports external plugins;
        //       until then allow-plugins is dropped.
        allow_plugins: IndexMap::new(),
    };

    let scripts = ScriptsSection {
        entries: manifest.scripts.clone(),
    };

    // This section holds arbitrary nested data (objects, arrays, scalars),
    // so convert each value as a real JSON -> TOML value.
    let extra = ExtraSection {
        data: manifest
            .extra
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| {
                TomlValue::try_from(v)
                    .ok()
                    .map(|value| (k.clone(), value))
            })
            .collect(),
    };

    let (repositories, sources) = build_sources(base, &manifest.repositories);

    PhastManifest {
        package,
        require,
        require_dev,
        provide: ProvideSection { entries: manifest.provide.clone() },
        replace: ReplaceSection { entries: manifest.replace.clone() },
        conflict: ConflictSection { entries: manifest.conflict.clone() },
        suggest: SuggestSection { entries: manifest.suggest.clone() },
        autoload,
        autoload_dev,
        config,
        repositories,
        sources,
        scripts,
        extra,
        ..Default::default()
    }
}

/// Collect credentials from all three sources into an `AuthConfig`.
///
/// Priority (highest to lowest): existing auth.toml > auth.json > inline composer.json config.
/// Missing files are silently skipped; the result is empty when all sources are empty.
pub fn build_auth_manifest(cwd: &Path, manifest: &ComposerManifest) -> Result<AuthConfig> {
    let auth_toml = cwd.join("auth.toml");
    let mut from_auth_toml = AuthConfig::load_from_toml_file(&auth_toml)?;

    let auth_json = cwd.join("auth.json");
    let from_auth_json = AuthConfig::load_from_json_file(&auth_json)?;
    let from_config = AuthConfig::from(&manifest.config);

    from_auth_toml.merge(from_auth_json);
    from_auth_toml.merge(from_config);
    Ok(from_auth_toml)
}

/// A parsed `composer.json` repository object.
type RepositoryObject = serde_json::Map<String, JsonValue>;

/// Split composer `repositories` into phast `[[repositories]]` endpoints and
/// per-package `[sources]` specs, dispatching each entry by its `type`:
///
/// - `path` -> `[sources]` `{ path, version? }`.
/// - inline `type: package` -> `[sources]` inline package.
/// - `composer` / `packagist` / `vcs` -> `[[repositories]]` endpoint.
#[rustfmt::skip]
fn build_sources(base: &Path, repositories: &[JsonValue]) -> (Vec<RepositoryDef>, SourcesSection) {
    let mut repos   = Vec::new();
    let mut sources = Vec::new();

    for repository in repositories {
        let Some(object) = repository
            .as_object() else
        {
            continue;
        };

        let Some(repo_type) = object
            .get("type")
            .and_then(JsonValue::as_str) else
        {
            continue;
        };

        match repo_type {
            "path"    => sources.extend(path_source(base, object)),
            "package" => sources.extend(inline_source(object)),
            other     => repos.extend(registry_repo(other, object)),
        }
    }

    (
        repos,
        SourcesSection {
            entries: sources
                .into_iter()
                .collect(),
        },
    )
}

/// A `type: path` repository as a `[sources]` `{ path, version? }` spec, keyed by
/// the package name read from the local directory's manifest.
#[rustfmt::skip]
fn path_source(base: &Path, object: &RepositoryObject) -> Option<(String, SourceSpec)> {
    let url = object
        .get("url")
        .and_then(JsonValue::as_str)?;
    let name = path_package_name(base, url)?;
    let version = object
        .get("options")
        .and_then(|options| options.get("versions"))
        .and_then(|versions| versions.get(name.as_str()))
        .and_then(JsonValue::as_str)
        .map(str::to_owned);

    let spec = SourceSpec {
        path: Some(url.to_owned()),
        version,
        ..SourceSpec::default()
    };
    Some((name, spec))
}

/// An inline `type: package` repository as a self-describing `[sources]` package.
#[rustfmt::skip]
fn inline_source(object: &RepositoryObject) -> Option<(String, SourceSpec)> {
    let package = object
        .get("package")
        .and_then(JsonValue::as_object)?;
    let name = package
        .get("name")
        .and_then(JsonValue::as_str)?;
    let source = package
        .get("source");

    let spec = SourceSpec {
        url:       str_field(source.and_then(|s| s.get("url"))),
        reference: str_field(source.and_then(|s| s.get("reference"))),
        version:   str_field(package.get("version")),
        ..SourceSpec::default()
    };
    Some((name.to_owned(), spec))
}

/// A registry (`composer` / `packagist`) or `vcs` discovery repository as a
/// `[[repositories]]` endpoint. `None` for an unrecognized `type`.
fn registry_repo(repo_type: &str, object: &RepositoryObject) -> Option<RepositoryDef> {
    Some(RepositoryDef {
        repo_type: repo_type.parse().ok()?,
        url: str_field(object.get("url")),
        name: None,
        default: false,
    })
}

/// The package name a `path` repository at `rel` exposes, read from its local
/// manifest (`phast.toml` or `composer.json`). `None` if it cannot be read.
fn path_package_name(base: &Path, rel: &str) -> Option<String> {
    Manifest::load(&base.join(rel)).ok()?.name
}

/// A JSON value read as an owned string, if present and a string.
fn str_field(value: Option<&JsonValue>) -> Option<String> {
    value.and_then(JsonValue::as_str).map(str::to_owned)
}
