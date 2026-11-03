use std::collections::{BTreeMap, HashMap};

use phast_autoload::render::{array, flat_ordered_map, flat_string_map, string, string_array_map};

use indexmap::IndexMap;

// ---------------------------------------------------------------------------
// Quoting a value as a PHP string literal
// ---------------------------------------------------------------------------

#[test]
fn string_quotes_a_plain_value() {
    assert_eq!(string("hello"), "'hello'");
}

#[test]
fn string_escapes_backslash() {
    assert_eq!(string("Foo\\Bar"), "'Foo\\\\Bar'");
}

#[test]
fn string_escapes_single_quote() {
    assert_eq!(string("it's"), "'it\\'s'");
}

#[test]
fn string_quotes_an_empty_value() {
    assert_eq!(string(""), "''");
}

#[test]
fn string_leaves_a_double_quote_alone() {
    // Single quotes are the delimiter, so a double quote needs no escape and
    // escaping it would put a literal backslash in the PHP string.
    assert_eq!(string("say \"hi\""), "'say \"hi\"'");
}

// ---------------------------------------------------------------------------
// A map of one string to one path, in the map's own key order
// ---------------------------------------------------------------------------

#[test]
fn flat_string_map_empty() {
    assert_eq!(flat_string_map(&BTreeMap::new()), "array()");
}

#[test]
fn flat_string_map_single_entry() {
    let map = BTreeMap::from([("Foo\\Bar".to_owned(), "foo/Bar.php".to_owned())]);
    let out = flat_string_map(&map);
    assert!(out.starts_with("array("));
    assert!(out.contains("'Foo\\\\Bar' => __DIR__ . '/../foo/Bar.php'"));
}

#[test]
fn flat_string_map_follows_the_maps_key_order() {
    let map = BTreeMap::from([
        ("Z\\Class".to_owned(), "/z.php".to_owned()),
        ("A\\Class".to_owned(), "/a.php".to_owned()),
    ]);
    let out = flat_string_map(&map);
    let pos_a = out.find("'A\\\\Class'").unwrap();
    let pos_z = out.find("'Z\\\\Class'").unwrap();

    // The renderer does not sort. Ordering is the `BTreeMap`'s doing, which is why
    // the classmap is stored in one rather than left to each renderer to arrange.
    assert!(pos_a < pos_z, "entries should be rendered in key order");
}

#[test]
fn flat_string_map_renders_one_entry_per_line() {
    let map = BTreeMap::from([
        ("A\\One".to_owned(), "a/One.php".to_owned()),
        ("B\\Two".to_owned(), "b/Two.php".to_owned()),
    ]);

    // The whole rendering, so indentation and the trailing comma are pinned
    // somewhere rather than only ever checked with `contains`.
    assert_eq!(
        flat_string_map(&map),
        "array(\n    'A\\\\One' => __DIR__ . '/../a/One.php',\n    \
         'B\\\\Two' => __DIR__ . '/../b/Two.php',\n)",
    );
}

#[test]
fn flat_string_map_escapes_a_quote_in_a_path() {
    let map = BTreeMap::from([("K".to_owned(), "it's/File.php".to_owned())]);

    assert_eq!(
        flat_string_map(&map),
        "array(\n    'K' => __DIR__ . '/../it\\'s/File.php',\n)",
    );
}

#[test]
fn flat_string_map_reaches_above_vendor_for_a_root_path() {
    let map = BTreeMap::from([("K".to_owned(), "../app".to_owned())]);

    // Values are vendor-relative and the expression is rooted at vendor/phast/,
    // so a root package path already starting with `../` doubles up here.
    assert_eq!(
        flat_string_map(&map),
        "array(\n    'K' => __DIR__ . '/../../app',\n)",
    );
}

// ---------------------------------------------------------------------------
// A map of prefixes to directory lists, longest prefix first
// ---------------------------------------------------------------------------

#[test]
fn string_array_map_empty() {
    assert_eq!(string_array_map(&HashMap::new()), "array()");
}

#[test]
fn string_array_map_single_entry() {
    let map = HashMap::from([(
        "Foo\\".to_owned(),
        vec!["foo/src".to_owned(), "foo/lib".to_owned()],
    )]);
    // Directories stay on the prefix's own line, in the order they were given:
    // PSR-4 resolution tries them in turn and the first hit wins.
    assert_eq!(
        string_array_map(&map),
        "array(\n    'Foo\\\\' => array(__DIR__ . '/../foo/src', __DIR__ . '/../foo/lib'),\n)",
    );
}

#[test]
fn string_array_map_same_length_sorted_alphabetically() {
    let map = HashMap::from([
        ("Z\\".to_owned(), vec!["/z".to_owned()]),
        ("A\\".to_owned(), vec!["/a".to_owned()]),
    ]);
    let out = string_array_map(&map);
    let pos_a = out.find("'A\\\\'").unwrap();
    let pos_z = out.find("'Z\\\\'").unwrap();
    assert!(
        pos_a < pos_z,
        "equal-length keys should be sorted alphabetically",
    );
}

#[test]
fn string_array_map_longer_prefix_before_shorter() {
    // PSR-4 correctness: a more specific prefix must appear before its parent
    // so the ClassLoader checks `Foo\Bar\` before `Foo\` and returns the right file.
    let map = HashMap::from([
        ("Foo\\".to_owned(), vec!["/vendor/foo/src".to_owned()]),
        (
            "Foo\\Bar\\".to_owned(),
            vec!["/vendor/foo-bar/src".to_owned()],
        ),
    ]);
    let out = string_array_map(&map);
    let pos_short = out.find("'Foo\\\\'").unwrap();
    let pos_long = out.find("'Foo\\\\Bar\\\\'").unwrap();
    assert!(
        pos_long < pos_short,
        "longer prefix 'Foo\\Bar\\' must appear before shorter 'Foo\\' in the output array",
    );
}

// ---------------------------------------------------------------------------
// A map whose insertion order is the point
// ---------------------------------------------------------------------------

#[test]
fn flat_ordered_map_empty() {
    assert_eq!(flat_ordered_map(&IndexMap::new()), "array()");
}

#[test]
fn flat_ordered_map_keeps_insertion_order() {
    // The only reason this exists beside flat_string_map: `files` entries must be
    // required in the order the builder folded them, so a package's files run after
    // those of its dependencies. Sorting here would break that.
    let mut map = IndexMap::new();
    map.insert("zeta".to_owned(), "z/z.php".to_owned());
    map.insert("alpha".to_owned(), "a/a.php".to_owned());

    let out = flat_ordered_map(&map);
    assert!(
        out.find("'zeta'").unwrap() < out.find("'alpha'").unwrap(),
        "insertion order must survive, not alphabetical order: {out}",
    );
}

// ---------------------------------------------------------------------------
// Wrapping lines another renderer already produced
// ---------------------------------------------------------------------------

#[test]
fn array_empty() {
    assert_eq!(array(&[]), "array()");
}

#[test]
fn array_wraps_pre_rendered_lines() {
    let elements = vec!["    'a',".to_owned(), "    'b',".to_owned()];

    assert_eq!(array(&elements), "array(\n    'a',\n    'b',\n)");
}
