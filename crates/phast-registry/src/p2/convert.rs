//! Turning Packagist's wire format into the domain model.
//!
//! The p2 structs mirror what a registry sends; the rest of Phast works in `phast-core`
//! types. These functions are the one-way crossing between the two, shared by every
//! provider so a field is read the same way whoever fetched it.

use std::collections::HashMap;

use phast_core::lock::phast::{AutoloadSection, ExtraSection};
use phast_core::package::PackageName;
use phast_version::{Constraint, RequireSpec, Stability, Version};

use anyhow::Context;
use serde_json::Value as JsonValue;

/// Derive the stability level of a resolved version.
///
/// Branch versions (e.g. `dev-main`) are always `Dev`; numeric versions
/// carry their stability in the parsed `NumericVersion` struct.
pub(crate) fn determine_version_stability(version: &Version) -> Stability {
    match version {
        Version::Numeric(n) => n.stability,
        Version::Branch(_) => Stability::Dev,
    }
}

/// Split a `vendor/package` name into its two components.
///
/// A name with no `/` yields `("_", name)`, which in practice means a platform
/// package such as `php` or `ext-json`.
pub(crate) fn split_package_name(name: &PackageName) -> (&str, &str) {
    name.as_str()
        .split_once('/')
        .unwrap_or(("_", name.as_str()))
}

/// Convert a raw dependency map (package name -> constraint string) into typed pairs.
///
/// Each value parses as a [`RequireSpec`] and resolves against `declaring`, so a
/// registry package's `self.version` requirement binds to the version being served.
/// Returns an error if any constraint string does not parse, naming the constraint
/// and the package that declared it.
pub(crate) fn convert_dependency_map(
    raw: &HashMap<String, String>,
    declaring: &Version,
) -> anyhow::Result<Vec<(PackageName, Constraint)>> {
    raw.iter()
        .map(|(name, constraint)| {
            let spec = constraint
                .parse::<RequireSpec>()
                .with_context(|| format!("invalid constraint '{constraint}' for '{name}'"))?;
            Ok((PackageName::new(name), spec.resolve(declaring)))
        })
        .collect()
}

/// Convert a p2 autoload map into an [`AutoloadSection`].
///
/// The p2 `autoload` field is a raw JSON map keyed by autoload type, and a value is a
/// string, an array of strings, or a map of prefix to path(s). A key carrying anything
/// other than the shape that key expects is skipped, so a malformed `psr-4` costs its
/// own entries and leaves the rest of the section intact.
pub(crate) fn convert_autoload(raw: &HashMap<String, JsonValue>) -> AutoloadSection {
    let mut result = AutoloadSection::default();

    if let Some(JsonValue::Object(psr0)) = raw.get("psr-0") {
        for (prefix, paths) in psr0 {
            result
                .psr0
                .insert(prefix.clone(), convert_to_str_vec(paths));
        }
    }

    if let Some(JsonValue::Object(psr4)) = raw.get("psr-4") {
        for (prefix, paths) in psr4 {
            result
                .psr4
                .insert(prefix.clone(), convert_to_str_vec(paths));
        }
    }

    if let Some(JsonValue::Array(classmap)) = raw.get("classmap") {
        result.classmap = classmap
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
    }

    if let Some(JsonValue::Array(files)) = raw.get("files") {
        result.files = files
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
    }

    // Composer nests `exclude-from-classmap` inside the autoload block; carry it
    // into the one internal exclude list so the classmap scanner honors it.
    if let Some(JsonValue::Array(exclude)) = raw.get("exclude-from-classmap") {
        result.exclude = exclude
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
    }

    result
}

/// Normalize a JSON value to a list of strings.
///
/// Accepts a single string or an array of strings. Any other shape yields an empty vec.
pub(crate) fn convert_to_str_vec(value: &JsonValue) -> Vec<String> {
    match value {
        JsonValue::String(s) => vec![s.clone()],
        JsonValue::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        _ => vec![],
    }
}

/// Convert a raw `extra` value into the lock's `extra` section.
///
/// Nulls are stripped (TOML has none) and an empty object yields `None`, so a package
/// that declares nothing leaves the field out rather than writing `extra = {}`.
pub(crate) fn convert_extra(value: JsonValue) -> Option<ExtraSection> {
    strip_nulls(value)
        .filter(|value| !matches!(value, JsonValue::Object(map) if map.is_empty()))
        .map(|data| ExtraSection { data })
}

/// Recursively remove `null` values from a JSON value so it can be serialized to TOML.
///
/// TOML has no null type, and the `toml` crate answers a `JsonValue::Null` with
/// `unsupported unit type` wherever it sits, nested or not.
///
/// - A top-level `null` becomes `None` (the caller drops the field entirely).
/// - Inside an object, keys whose values are `null` are removed.
/// - Inside an array, `null` elements are removed.
#[rustfmt::skip]
pub(crate) fn strip_nulls(value: JsonValue) -> Option<JsonValue> {
    match value {
        JsonValue::Null => None,
        JsonValue::Object(obj) => {
            let cleaned = obj
                .into_iter()
                .filter_map(|(k, v)| strip_nulls(v).map(|v| (k, v)))
                .collect();
            Some(JsonValue::Object(cleaned))
        }
        JsonValue::Array(arr) => {
            let cleaned = arr
                .into_iter()
                .filter_map(strip_nulls)
                .collect();
            Some(JsonValue::Array(cleaned))
        }
        other => Some(other),
    }
}
