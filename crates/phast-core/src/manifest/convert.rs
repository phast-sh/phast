//! Turning the two serde surfaces into the domain model.
//!
//! Both `ComposerManifest::into_manifest()` and `PhastManifest::into_manifest()` have to
//! parse the same primitives out of strings: version constraints, repository entries,
//! stability names, durations. Those parsers live here so the two formats cannot drift
//! in how they read a value they both accept.
//!
//! Private to `phast-core`.

use super::phast::RepositoryDef;
use super::{PlatformCheck, Repository, RepositoryType};
use crate::package::PackageName;

use std::collections::HashMap;
use std::fmt::Display;
use std::time::Duration;

use phast_version::{RequireSpec, Stability, VersionSpec};

use serde_json::{Map, Value as JsonValue};
use toml::Value as TomlValue;

/// Normalize a `serde_json::Value` PSR-4/PSR-0 path value to `Vec<String>`.
///
/// Accepts either a single string or an array of strings. Any other shape
/// returns `Err(description)`.
pub(super) fn normalize_json_paths(value: &JsonValue) -> Result<Vec<String>, String> {
    match value {
        JsonValue::String(s) => Ok(vec![s.clone()]),
        JsonValue::Array(arr) => arr.iter().map(|v| expect_str(v.as_str(), v)).collect(),
        other => Err(format!(
            "autoload path must be a string or array of strings, got: {}",
            other,
        )),
    }
}

/// Normalize a `toml::Value` PSR-4/PSR-0 path value to `Vec<String>`.
///
/// Accepts either a single string or an array of strings. Any other shape
/// returns `Err(description)`.
pub(super) fn normalize_toml_paths(value: &TomlValue) -> Result<Vec<String>, String> {
    match value {
        TomlValue::String(s) => Ok(vec![s.clone()]),
        TomlValue::Array(arr) => arr.iter().map(|v| expect_str(v.as_str(), v)).collect(),
        other => Err(format!(
            "autoload path must be a string or array of strings, got: {}",
            other,
        )),
    }
}

/// Parse a raw `require` / `require-dev` / `conflict` map into typed form.
///
/// Keys are normalized to lowercase via `PackageName`. Values become
/// [`RequireSpec`], so `self.version` is accepted (resolved later against the
/// declaring package's version) rather than rejected. If any value fails to
/// parse, returns `Err((package_name, reason))`.
#[rustfmt::skip]
pub(super) fn parse_require_map(
    map: impl IntoIterator<Item = (String, String)>,
) -> Result<HashMap<PackageName, RequireSpec>, (String, String)> {
    map.into_iter()
        .map(|(name, value)| {
            let pkg  = PackageName::new(&name);
            let spec = value
                .parse::<RequireSpec>()
                .map_err(|e| (name, e.to_string()))?;
            Ok((pkg, spec))
        })
        .collect()
}

/// Parse a raw `replace` / `provide` map into typed [`VersionSpec`] values.
///
/// Values are version specs (`self.version`, `*`, or an exact version), not constraints.
/// A value that does not parse, a version *range* being the rare case, is skipped rather
/// than failing the whole manifest, so one odd entry cannot make a project unloadable.
pub(super) fn parse_version_spec(
    map: impl IntoIterator<Item = (String, String)>,
) -> HashMap<PackageName, VersionSpec> {
    map.into_iter()
        .filter_map(|(name, spec)| {
            Some((PackageName::new(&name), spec.parse::<VersionSpec>().ok()?))
        })
        .collect()
}

/// Parse an optional `minimum-stability` string into a `Stability` value.
///
/// `None`    - `Ok(Stability::Stable)`.
/// `Some(s)` - parses `s` or returns `Err(unknown_label)`.
pub(super) fn parse_stability(string: Option<&str>) -> Result<Stability, String> {
    let Some(stability) = string else {
        return Ok(Stability::Stable);
    };

    Stability::from_label(stability).ok_or_else(|| stability.to_owned())
}

/// Parse an optional `platform-check` string into a [`PlatformCheck`] value.
///
/// `None`    - `Ok(PlatformCheck::Php)` (the default).
/// `Some(s)` - the mapping below, or `Err` with the invalid value.
pub(super) fn parse_platform_check(string: Option<&str>) -> Result<PlatformCheck, String> {
    match string {
        None | Some("php") => Ok(PlatformCheck::Php),
        Some("all") => Ok(PlatformCheck::All),
        Some("off") => Ok(PlatformCheck::Off),
        Some(other) => Err(format!(r#"expected "php", "all", or "off", got "{other}""#,)),
    }
}

/// Parse an optional duration string of the form `"24h"`, `"7d"`, or `"2w"`.
///
/// `None`    - `Ok(None)`.
/// `Some(s)` - parses `s` and returns `Ok(Some(duration))` or `Err`.
pub(super) fn parse_duration(string: Option<&str>) -> Result<Option<Duration>, String> {
    let Some(duration) = string else {
        return Ok(None);
    };

    let duration = duration.trim();
    let (digits, unit) = duration
        .find(|c: char| c.is_alphabetic())
        .map(|i| (&duration[..i], &duration[i..]))
        .ok_or_else(|| format!("missing unit in duration '{duration}': expected h, d, or w"))?;

    let number: u64 = digits
        .parse()
        .map_err(|_| format!("invalid number in duration '{duration}'"))?;

    let secs = match unit.to_lowercase().as_str() {
        "h" => number * 3600,
        "d" => number * 86400,
        "w" => number * 7 * 86400,
        other => {
            return Err(format!(
                "unknown unit '{other}' in duration '{duration}': expected h, d, or w",
            ));
        }
    };

    Ok(Some(Duration::from_secs(secs)))
}

/// Parse a typed `RepositoryDef` (from `phast.toml`) into a `Repository`.
///
/// Every `RepositoryType` is matched, so `None` never means the type was unknown. It means
/// the entry was understood and yielded no repository, which is the inline `package` case.
#[rustfmt::skip]
pub(super) fn parse_toml_repository(
    repository: &RepositoryDef,
) -> Result<Option<Repository>, String> {
    match repository.repo_type {
        RepositoryType::Vcs |
        RepositoryType::Git => {
            let url = repository.url.clone().ok_or_else(|| "vcs repository missing 'url'".to_owned())?;
            Ok(
                Some(Repository::Vcs {
                    url,
                    name: repository.name.clone(),
                }),
            )
        },
        RepositoryType::Composer => {
            let url = repository.url.clone().ok_or_else(|| "composer repository missing 'url'".to_owned())?;
            Ok(
                Some(Repository::Composer {
                    url,
                    name: repository.name.clone(),
                    default: repository.default,
                }),
            )
        },
        RepositoryType::Packagist => {
            Ok(
                Some(Repository::Packagist {
                    url: repository.url.clone(),
                    name: repository.name.clone(),
                    default: repository.default,
                }),
            )
        },
        RepositoryType::Path => {
            let path = repository.url.clone().ok_or_else(|| "path repository missing 'url'".to_owned())?;
            Ok(
                Some(Repository::Path {
                    path,
                    options: HashMap::new(),
                }),
            )
        },
         // TODO: not implemented, so phast.toml accepts an inline package and ignores it
        RepositoryType::Package => {
            Ok(
                None,
            )
        },
    }
}

/// Parse a single repository JSON object into a `Repository`.
///
/// Returns `None` for unknown types (forward-compatible skip)
/// and `Err(reason)` for malformed entries (missing `type`, missing required fields).
#[rustfmt::skip]
pub(super) fn parse_json_repository(
    repository: JsonValue,
) -> Result<Option<Repository>, String> {
    let object = repository
        .as_object()
        .ok_or_else(|| "repository entry must be a JSON object".to_owned())?;

    let type_value = object
        .get("type")
        .ok_or_else(|| "repository entry missing required 'type' field".to_owned())?;

    // Unknown types are skipped for forward compatibility.
    let Ok(repo_type) = serde_json::from_value::<RepositoryType>(type_value.clone()) else {
        return Ok(None);
    };

    match repo_type {
        RepositoryType::Vcs |
        RepositoryType::Git => {
            let url = require_str(object, "url", "vcs repository")?;
            Ok(
                Some(Repository::Vcs {
                    url,
                    name: None,
                }),
            )
        },
        RepositoryType::Composer => {
            let url = require_str(object, "url", "composer repository")?;
            Ok(
                Some(Repository::Composer {
                    url,
                    name: None,
                    default: false,
                }),
            )
        },
        RepositoryType::Packagist => {
            let url = object.get("url").and_then(|v| v.as_str()).map(str::to_owned);
            Ok(
                Some(Repository::Packagist {
                    url,
                    name: None,
                    default: false,
                }),
            )
        },
        RepositoryType::Path => {
            let path = require_str(object, "url", "path repository")?;
            let options = collect_options(object);
            Ok(
                Some(Repository::Path {
                    path,
                    options,
                }),
            )
        },
        RepositoryType::Package => {
            Ok(
                Some(Repository::Package {
                    package: repository,
                }),
            )
        },
    }
}

/// Unwrap an `Option<&str>` or return an error quoting the original value.
fn expect_str(string: Option<&str>, display: &impl Display) -> Result<String, String> {
    string
        .map(str::to_owned)
        .ok_or_else(|| format!("expected string in path array, got: {}", display))
}

/// Extract a required string field from a JSON object, or return a descriptive error.
fn require_str(
    object: &Map<String, JsonValue>,
    field: &str,
    context: &str,
) -> Result<String, String> {
    object
        .get(field)
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("{} missing '{}'", context, field))
}

/// Collect all repository object fields except `type` and `url` into an options map.
fn collect_options(object: &Map<String, JsonValue>) -> HashMap<String, JsonValue> {
    object
        .iter()
        .filter(|(k, _)| k.as_str() != "type" && k.as_str() != "url")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
