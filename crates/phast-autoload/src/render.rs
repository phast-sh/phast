//! Rendering PHP literals.
//!
//! Pure string-building: every function here turns Rust data into the text of a PHP
//! value. Nothing in this module touches the filesystem; writing the rendered text
//! is `writer`'s job.

use std::collections::{BTreeMap, HashMap};

use indexmap::IndexMap;

/// Render a `HashMap<String, Vec<String>>` as a PHP `array('key' => array(...), ...)`.
///
/// Entries are sorted by descending key length, then alphabetically for ties.
/// Longest-first ordering ensures that PSR-4 and PSR-0 loaders check more specific
/// prefixes before shorter ones that share the same root (e.g. `Foo\Bar\` before `Foo\`).
pub fn string_array_map(map: &HashMap<String, Vec<String>>) -> String {
    let mut map: Vec<_> = map.iter().collect();
    map.sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()).then(a.cmp(b)));

    if map.is_empty() {
        return "array()".to_string();
    }

    let entries = map
        .iter()
        .map(|(namespace, paths)| {
            let path_list = paths
                .iter()
                .map(|p| dir_path(p))
                .collect::<Vec<_>>()
                .join(", ");
            format!("    {} => array({}),", string(namespace), path_list)
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("array(\n{}\n)", entries)
}

/// Render a `BTreeMap<String, String>` as a PHP `array('key' => 'value', ...)`.
///
/// Key order comes from the map, which is why the classmap is a `BTreeMap`: the
/// ordering guarantee lives in the data rather than in every renderer that touches it.
pub fn flat_string_map(map: &BTreeMap<String, String>) -> String {
    if map.is_empty() {
        return "array()".to_string();
    }

    let entries = map
        .iter()
        .map(|(k, v)| format!("    {} => {},", string(k), dir_path(v)))
        .collect::<Vec<_>>()
        .join("\n");

    format!("array(\n{}\n)", entries)
}

/// Render an `IndexMap<String, String>` as a PHP `array(...)`, preserving insertion order.
pub fn flat_ordered_map(map: &IndexMap<String, String>) -> String {
    if map.is_empty() {
        return "array()".to_string();
    }

    let entries = map
        .iter()
        .map(|(k, v)| format!("    {} => {},", string(k), dir_path(v)))
        .collect::<Vec<_>>()
        .join("\n");

    format!("array(\n{}\n)", entries)
}

/// Wrap pre-rendered element lines in a PHP `array(...)`, or `array()` when empty.
pub fn array(elements: &[String]) -> String {
    if elements.is_empty() {
        return "array()".to_string();
    }

    format!("array(\n{}\n)", elements.join("\n"))
}

/// Wrap `value` in single quotes, escaping backslashes and single quotes.
pub fn string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Render a vendor-relative path as a PHP `__DIR__`-relative expression.
///
/// Paths stored as vendor-relative (e.g. `pkg/src/file.php` or `../app`) become
/// `__DIR__ . '/../pkg/src/file.php'` or `__DIR__ . '/../../app'`, which resolve
/// correctly at PHP parse time regardless of where the vendor dir is mounted.
fn dir_path(rel: &str) -> String {
    format!("__DIR__ . '/../{}'", rel.replace('\'', "\\'"))
}
