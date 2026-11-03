//! Formatter for `phast.toml` output.
//!
//! The `toml` serializer emits valid TOML that is not the house style: literal and
//! triple-quoted strings where a plain one would do, a sub-table where an inline one reads
//! better, and no ordering. Everything here rewrites that output rather than replacing the
//! serializer, so the shape of a manifest is decided in one place.

use crate::package;
use crate::toml as phast_toml;

use anyhow::Result;
use toml_edit::DocumentMut as TomlEditDocument;
use toml_edit::InlineTable as TomlEditInlineTable;
use toml_edit::Item as TomlEditItem;
use toml_edit::Value as TomlEditValue;

/// Controls how `format` post-processes a serialized `phast.toml`.
#[derive(Debug, Clone, Default)]
pub struct FormatOptions {
    /// When `true`, sort every keyed dependency section by the tiers [`sort_rank`] assigns,
    /// platform packages first, then alphabetically within a tier.
    pub sort_packages: bool,
}

/// Post-process raw TOML serialized from a `PhastManifest`.
///
/// Fixes string encoding artifacts from the `toml` crate serializer,
/// reformats script arrays, and optionally sorts dependency sections.
#[rustfmt::skip]
pub fn format(toml: &str, options: &FormatOptions) -> Result<String> {
    let mut document: TomlEditDocument = phast_toml::parse_document(toml)?;

    phast_toml::normalize_strings(document.as_table_mut());
    phast_toml::normalize_spacing(&mut document);
    phast_toml::normalize_assignments(document.as_table_mut());

    reformat_arrays(&mut document, "scripts");
    reformat_tables(&mut document, "sources");

    if options.sort_packages {
        // All keyed dependency sections sort by the same rule (platform packages
        // first, then alphabetically). `conflict` legitimately carries platform
        // packages too; the rest simply have none and fall through to alphabetical.
        for section in [
            "require",
            "require-dev",
            "provide",
            "replace",
            "conflict",
            "suggest",
            "sources",
        ] {
            sort_section(&mut document, section);
        }
    }

    Ok(document.to_string())
}

/// Reformat all values in `[scripts]` as multiline arrays with no trailing commas.
///
/// Single-element arrays are also expanded to multiline for consistency.
fn reformat_arrays(toml: &mut TomlEditDocument, section: &str) {
    let Some(TomlEditItem::Table(scripts)) = toml.get_mut(section) else {
        return;
    };

    for (_, script) in scripts.iter_mut() {
        let TomlEditItem::Value(TomlEditValue::Array(arr)) = script else {
            continue;
        };

        arr.set_trailing_comma(false);
        arr.set_trailing("\n");

        for v in arr.iter_mut() {
            let TomlEditValue::String(f) = v else {
                continue;
            };

            f.decor_mut().set_prefix("\n    ");
            f.decor_mut().set_suffix("");
        }
    }
}

/// Collapse a section's sub-table entries into inline tables, so each
/// `[sources."vendor/pkg"]` block renders as `"vendor/pkg" = { ... }` under a
/// single `[sources]` header (the house style for per-package sources).
fn reformat_tables(toml: &mut TomlEditDocument, section: &str) {
    let Some(TomlEditItem::Table(table)) = toml.get_mut(section) else {
        return;
    };

    let keys: Vec<String> = table
        .iter()
        .filter(|(_, item)| item.is_table())
        .map(|(key, _)| key.to_owned())
        .collect();

    for key in keys {
        let Some(TomlEditItem::Table(sub)) = table.get(&key) else {
            continue;
        };

        let mut inline: TomlEditInlineTable = sub
            .iter()
            .filter_map(|(k, v)| Some((k, v.as_value()?.clone())))
            .collect();
        // Every value still carries the spacing it had as a sub-table entry, where
        // nothing followed it on its line. Clearing that lets the inline-table
        // defaults apply, so the closing brace is spaced like the opening one.
        inline.fmt();

        table.insert(
            &key,
            TomlEditItem::Value(TomlEditValue::InlineTable(inline)),
        );
    }

    // Entries are inline values now, so the section renders its own `[sources]`
    // header rather than only per-entry sub-table headers.
    table.set_implicit(false);
}

/// Sort a toml table by dependency tier, then alphabetically within a tier.
#[rustfmt::skip]
fn sort_section(toml: &mut TomlEditDocument, section: &str) {
    let Some(TomlEditItem::Table(t)) = toml.get_mut(section) else {
        return;
    };

    t.sort_values_by(|a, _, b, _| {
        sort_rank(a)
            .cmp(&sort_rank(b))
            .then_with(|| a.cmp(b))
        });
}

/// Ordering tier for a dependency name:
///
/// 0. the PHP runtime (`php`, `php-*`)
/// 1. extensions (`ext-*`)
/// 2. other platform constraints (`lib-*`, `composer-*`)
/// 3. regular packages
///
/// Vendor packages that merely look platform-like (e.g. `php-http/discovery`,
/// which contains a `/`) are not platform and fall to tier 3.
fn sort_rank(name: &str) -> u8 {
    if package::is_php(name) {
        return 0;
    }
    if package::is_ext(name) {
        return 1;
    }
    if package::is_platform(name) {
        return 2;
    }
    3
}
