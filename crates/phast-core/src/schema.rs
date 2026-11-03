//! JSON Schema generation for `phast.toml`, behind the `schema` feature.
//!
//! The shape is derived from the `PhastManifest` serde types with `schemars`, so it tracks
//! the deserializer for free. The `schema_with` overrides below are the exception: each
//! narrows a field the types declare as a plain string, and that narrowing holds only for as
//! long as someone edits it alongside the parser reading the field. The schema can therefore
//! reject a manifest Phast accepts.
//!
//! It is a build artifact. The `schema` binary writes it to
//! `schema/v{version}/manifest.json`, which is committed and served verbatim at
//! [`meta::schema_url`]. Nothing at runtime reads it: it exists for the editor that follows
//! the `#:schema` directive atop a generated manifest.

use crate::manifest::phast::PhastManifest;
use crate::meta;

use schemars::{Schema, SchemaGenerator, json_schema, schema_for};
use serde_json::{to_string_pretty, to_value};

/// Generate the `phast.toml` JSON Schema as a pretty-printed string.
///
/// `schemars` supplies `$schema` (Draft 2020-12) and titles the document after the Rust type,
/// which is the name an editor shows, so `Phast` replaces it. `$id` is added so the document
/// is self-describing at the versioned URL it is served from.
#[rustfmt::skip]
pub fn generate() -> anyhow::Result<String> {
    let mut root = to_value(schema_for!(PhastManifest))?;

    root["$id"] = meta::schema_url()
        .into();
    root["title"] = "Phast"
        .into();

    Ok(to_string_pretty(&root)?)
}

/// Schema for `config.minimum-stability`: the canonical spelling of each stability level,
/// offered as completions instead of accepting any string. The parser is looser, taking
/// aliases (`a`, `b`, `pl`, `patch`) and any casing.
pub(crate) fn stability_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "string",
        "enum": ["stable", "rc", "beta", "alpha", "dev"]
    })
}

/// Schema for `config.minimum-package-age`: a duration like `24h`, `7d`, or `2w`,
/// so the schema validates the shape instead of accepting any string.
/// The unit is case-insensitive.
pub(crate) fn duration_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "string",
        "pattern": "^[0-9]+[hdwHDW]$"
    })
}

/// Schema for a `psr-4` / `psr-0` map: each namespace prefix maps to a single path or a
/// list of paths. The real fields are `toml::Value` (which has no `JsonSchema` impl),
/// so `#[schemars(schema_with = ...)]` substitutes this shape.
pub(crate) fn psr_map_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "additionalProperties": {
            "anyOf": [
                { "type": "string" },
                { "type": "array", "items": { "type": "string" } }
            ]
        }
    })
}
