//! Low-level `toml_edit` helpers for in-place TOML manipulation.
//!
//! These functions are format-agnostic and work on any TOML document.
//! Manifest-specific formatting lives in `manifest::formatter`.

use anyhow::{Context, Result};
use serde_json::Value as JsonValue;
use toml::Value as TomlValue;
use toml_edit::Decor as TomlEditDecor;
use toml_edit::DocumentMut as TomlEditDocument;
use toml_edit::Item as TomlEditItem;
use toml_edit::Key as TomlEditKey;
use toml_edit::Table as TomlEditTable;
use toml_edit::Value as TomlEditValue;

/// Convert a PSR-4/PSR-0 path value from JSON to TOML.
///
/// Composer allows either a single string or an array of strings as the path
/// value. Both map to a TOML array so `phast.toml` holds one shape. Anything
/// else, and any non-string element of an array, is dropped rather than reported.
#[rustfmt::skip]
pub fn from_json(value: &JsonValue) -> TomlValue {
    match value {
        JsonValue::String(s) => TomlValue::Array(vec![TomlValue::String(s.clone())]),
        JsonValue::Array(arr) => {
            let items = arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| TomlValue::String(s.to_owned()))
                .collect();
            TomlValue::Array(items)
        }
        _ => TomlValue::Array(vec![]),
    }
}

/// Parse a raw TOML string into an editable `toml_edit` document.
#[rustfmt::skip]
pub fn parse_document(toml: &str) -> Result<TomlEditDocument> {
    let document: TomlEditDocument = toml
        .parse()
        .context("failed to parse TOML document")?;
    Ok(document)
}

/// Normalize all strings in a raw TOML document to basic strings,
/// preserving comments and key order. See [`normalize_strings`].
pub fn normalize_document(toml: &str) -> Result<String> {
    let mut document: TomlEditDocument = parse_document(toml)?;
    normalize_strings(document.as_table_mut());
    Ok(document.to_string())
}

/// Ensure exactly one blank line before every section header (`[section]` or
/// `[[section]]`), keeping the comment lines that belong to it.
///
/// A header loses that blank line only when it opens the document. One preceded
/// by root-level keys keeps it, so `version = 1` stays separated from `[package]`.
#[rustfmt::skip]
pub fn normalize_spacing(document: &mut TomlEditDocument) {
    let mut first = document
        .as_table()
        .iter()
        .all(|(_, item)| !item.is_value());
    normalize_table_spacing(document.as_table_mut(), &mut first);
}

/// Recursively rewrite the decor prefix of every section header.
///
/// `first` tracks whether the next header opens the document and therefore needs
/// no leading blank line. An implicit table (`[a.b]` written without `[a]`) and a
/// dotted one print no header of their own, so they leave `first` alone and only
/// their contents are walked.
fn normalize_table_spacing(table: &mut TomlEditTable, first: &mut bool) {
    for (_, item) in table.iter_mut() {
        match item {
            TomlEditItem::Table(t) => {
                if !t.is_implicit() && !t.is_dotted() {
                    normalize_header_prefix(t.decor_mut(), *first);
                    *first = false;
                }
                normalize_table_spacing(t, first);
            }
            TomlEditItem::ArrayOfTables(at) => {
                for t in at.iter_mut() {
                    normalize_header_prefix(t.decor_mut(), *first);
                    *first = false;
                    normalize_table_spacing(t, first);
                }
            }
            _ => {}
        }
    }
}

/// Rewrite a section header's prefix to a single blank line (or nothing for
/// the first header), keeping its comment lines.
///
/// Every comment line in the prefix survives and is moved flush against the header,
/// so a comment the author separated from it by a blank line ends up touching it.
fn normalize_header_prefix(decor: &mut TomlEditDecor, first: bool) {
    let prefix = decor
        .prefix()
        .and_then(|p| p.as_str())
        .unwrap_or_default()
        .to_string();

    let mut new_prefix = String::new();
    if !first {
        new_prefix.push('\n');
    }
    for line in prefix.lines().filter(|l| l.trim_start().starts_with('#')) {
        new_prefix.push_str(line.trim_start());
        new_prefix.push('\n');
    }

    decor.set_prefix(new_prefix);
}

/// Recursively normalize the whitespace around `=` to exactly one space on both sides.
///
/// Reaches the key-value pairs of tables and arrays of tables. The interior of an
/// inline table is left as written, so `y = {p   =   2}` keeps its inner spacing.
#[rustfmt::skip]
pub fn normalize_assignments(table: &mut TomlEditTable) {
    let keys: Vec<String> = table
        .iter()
        .filter(|(_, item)| item.is_value())
        .map(|(k, _)| k.to_string())
        .collect();

    for key in &keys {
        if let Some(mut k) = table.key_mut(key) {
            k.leaf_decor_mut().set_suffix(" ");
        }
        if let Some(TomlEditItem::Value(v)) = table.get_mut(key) {
            v.decor_mut().set_prefix(" ");
        }
    }

    for (_, item) in table.iter_mut() {
        match item {
            TomlEditItem::Table(t) => normalize_assignments(t),
            TomlEditItem::ArrayOfTables(at) => {
                for t in at.iter_mut() {
                    normalize_assignments(t);
                }
            }
            _ => {}
        }
    }
}

/// Recursively walk a table and rewrite literal (`'...'`) and triple-quoted
/// (`"""..."""`) strings into properly escaped basic strings (`"..."`).
///
/// A value takes either form; a key can only be literal, because TOML has no
/// triple-quoted key, so that branch tests only for the quote.
///
/// The `toml` serializer emits literal strings for values containing backslashes
/// (e.g. PHP namespaces like `App\Models`) and triple-quoted basic strings for
/// values that contain both `"` and `'`. Both forms are valid TOML but inconsistent
/// with the basic string style used everywhere else in `phast.toml`.
#[rustfmt::skip]
pub fn normalize_strings(table: &mut TomlEditTable) {
    let keys: Vec<String> = table
        .iter()
        .map(|(k, _)| k.to_string())
        .collect();

    for key in keys {
        if let Some(item) = table.get_mut(&key) {
            normalize_item(item);
        }

        let is_literal_key = table
            .key(&key)
            .and_then(|k| k.as_repr())
            .and_then(|r| r.as_raw().as_str())
            .is_some_and(|r| r.starts_with('\''));
        if !is_literal_key {
            continue;
        }

        let Some((_old, item)) = table.remove_entry(&key) else {
            continue;
        };

        table.insert_formatted(&make_basic_key(&key), item);
    }
}

/// Dispatch normalization to the appropriate handler for each `Item` variant.
fn normalize_item(item: &mut TomlEditItem) {
    match item {
        TomlEditItem::Value(v) => normalize_value(v),
        TomlEditItem::Table(t) => normalize_strings(t),
        TomlEditItem::ArrayOfTables(at) => {
            for t in at.iter_mut() {
                normalize_strings(t);
            }
        }
        TomlEditItem::None => {}
    }
}

/// Replace literal or triple-quoted string values with basic strings,
/// recursing into arrays and inline tables.
#[rustfmt::skip]
fn normalize_value(value: &mut TomlEditValue) {
    match value {
        TomlEditValue::String(formatted) => {
            let r = formatted
                .as_repr()
                .and_then(|r| r.as_raw().as_str())
                .unwrap_or_default();
            if !r.starts_with('\'') &&
               !r.starts_with("\"\"\"")
            {
                return;
            }

            let mut new_val = make_basic_string(formatted.value());
            // Preserve decor (whitespace prefix/suffix) so multiline array
            // elements stay on their own lines after replacement.
            if let TomlEditValue::String(ref mut string) = new_val {
                *string.decor_mut() = formatted.decor().clone();
            }
            *value = new_val;
        }
        TomlEditValue::Array(arr) => {
            arr.set_trailing_comma(false);
            for value in arr.iter_mut() {
                normalize_value(value);
            }
        }
        TomlEditValue::InlineTable(t) => {
            for (_, value) in t.iter_mut() {
                normalize_value(value);
            }
        }
        _ => {}
    }
}

/// Create a `TomlEditValue::String` with a properly escaped basic string repr.
///
/// `toml_edit` does not expose a public API for setting a value's raw repr
/// directly. The workaround is to parse a throwaway snippet (`x = "..."`) so
/// the parser produces a `Value::String` with the desired repr already baked in,
/// then clone the value out of the temporary document.
fn make_basic_string(string: &str) -> TomlEditValue {
    let escaped = string
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    let formatted = format!("x = \"{escaped}\"");

    let document: TomlEditDocument = formatted
        .parse()
        .expect("escaped string should produce valid TOML");
    document["x"]
        .as_value()
        .expect("x key should be a value")
        .clone()
}

/// Create a `TomlEditKey` with a properly escaped basic string repr.
///
/// Uses the same snippet-parse approach as [`make_basic_string`]: wraps the
/// escaped string in quotes and parses it as a standalone TOML key so the
/// resulting `Key` carries the correct repr when inserted into a document.
#[rustfmt::skip]
fn make_basic_key(string: &str) -> TomlEditKey {
    let escaped = string
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let formatted = format!("\"{}\"", escaped);

    TomlEditKey::parse(&formatted)
        .expect("escaped key should be valid TOML")
        .into_iter()
        .next()
        .expect("key parse should yield at least one key")
}
