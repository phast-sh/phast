use phast_core::manifest::phast::PhastManifest;

use indoc::indoc;
use toml_edit::DocumentMut;

/// A `[sources]` block in the shape the formatter must produce: one header, and one
/// inline table per entry. It is both the expectation below and an input in its own
/// right, so no test can assert a shape its own fixture never had.
const SOURCES: &str = indoc! {r#"
    [sources]
    "acme/contracts" = { path = "vendor-local/contracts", version = "3.7.x-dev" }
    "vendor/uuid" = { url = "https://github.com/vendor/uuid", reference = "abc123", version = "1.4.0" }
"#};

/// Keys of `section`, in the order they appear in `formatted`.
///
/// Parsed with `toml_edit`, which preserves document order (unlike the `toml`
/// deserializer, which sorts keys).
fn keys(formatted: &str, section: &str) -> Vec<String> {
    formatted.parse::<DocumentMut>().unwrap()[section]
        .as_table()
        .unwrap()
        .iter()
        .map(|(key, _)| key.to_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// Ordering a dependency section
// ---------------------------------------------------------------------------

#[test]
fn sort_orders_require_by_tier_then_alphabetically() {
    let input = indoc! {r#"
        [require]
        "vendor/z"             = "^1.0"
        "ext-json"             = "*"
        "php-http/discovery"   = "^1.0"
        "php"                  = ">=8.2"
        "composer-runtime-api" = ">=2.0"
        "acme/a"               = "^2.0"
        "php-64bit"            = "*"
        "lib-curl"             = "*"
        "ext-mbstring"         = "*"

        [config]
        sort-packages = true
    "#};

    let output = PhastManifest::format(input).unwrap();

    assert_eq!(
        keys(&output, "require"),
        [
            // tier 0: the PHP runtime
            "php",
            "php-64bit",
            // tier 1: extensions
            "ext-json",
            "ext-mbstring",
            // tier 2: other platform constraints
            "composer-runtime-api",
            "lib-curl",
            // tier 3: regular packages. `php-http/discovery` has a slash, so it is a
            // regular package rather than the PHP runtime.
            "acme/a",
            "php-http/discovery",
            "vendor/z",
        ],
    );
}

#[test]
fn sorting_reaches_every_keyed_section() {
    let input = indoc! {r#"
        [require-dev]
        "vendor/z" = "^1.0"
        "php"      = ">=8.2"

        [conflict]
        "vendor/b" = "<1.0"
        "ext-json" = "*"

        [sources]
        "zzz/last"  = { path = "z" }
        "aaa/first" = { path = "a" }

        [config]
        sort-packages = true
    "#};

    let output = PhastManifest::format(input).unwrap();

    // Not just `[require]`: the same tiering runs over every keyed section, which
    // is why `conflict` puts its platform entry first rather than sorting plainly.
    assert_eq!(keys(&output, "require-dev"), ["php", "vendor/z"]);
    assert_eq!(keys(&output, "conflict"), ["ext-json", "vendor/b"]);
    assert_eq!(keys(&output, "sources"), ["aaa/first", "zzz/last"]);
}

#[test]
fn require_keeps_its_order_when_sort_packages_is_off() {
    let input = indoc! {r#"
        [require]
        "vendor/z" = "^1.0"
        "php"      = ">=8.2"
        "acme/a"   = "^2.0"
    "#};

    let output = PhastManifest::format(input).unwrap();

    assert_eq!(keys(&output, "require"), ["vendor/z", "php", "acme/a"]);
}

// ---------------------------------------------------------------------------
// Rendering [sources] as inline tables
// ---------------------------------------------------------------------------

/// Formatting an already-formatted block must be a no-op, or `fmt --check` would report
/// a file it had written itself as needing changes.
#[test]
fn formatting_an_inline_sources_block_is_idempotent() {
    assert_eq!(PhastManifest::format(SOURCES).unwrap(), SOURCES);
}

/// Sub-tables are the shape the `toml` serializer emits and the one a handwritten
/// manifest may use; both must come out as inline entries under a single header.
#[test]
fn sources_render_as_inline_tables() {
    let input = indoc! {r#"
        [sources."acme/contracts"]
        path = "vendor-local/contracts"
        version = "3.7.x-dev"

        [sources."vendor/uuid"]
        url = "https://github.com/vendor/uuid"
        reference = "abc123"
        version = "1.4.0"
    "#};

    let output = PhastManifest::format(input).unwrap();

    assert_eq!(output, SOURCES);
}

// ---------------------------------------------------------------------------
// Rewriting what the serializer emitted
// ---------------------------------------------------------------------------

#[test]
fn scripts_become_multiline_arrays() {
    let input = indoc! {r#"
        [scripts]
        test = ["phpunit"]
        lint = ["phpcs src/", "phpstan analyse"]
    "#};

    let output = PhastManifest::format(input).unwrap();

    // One entry per line whatever the length, so adding a step to a script is a
    // one-line diff. A single-element array is expanded for the same reason.
    assert_eq!(
        output,
        indoc! {r#"
            [scripts]
            test = [
                "phpunit"
            ]
            lint = [
                "phpcs src/",
                "phpstan analyse"
            ]
        "#},
    );
    assert_eq!(PhastManifest::format(&output).unwrap(), output);
}

#[test]
fn literal_and_triple_quoted_strings_become_plain() {
    let input = indoc! {r#"
        [package]
        name = 'acme/app'
        description = """some description"""
    "#};

    let output = PhastManifest::format(input).unwrap();

    // The `toml` serializer reaches for these forms on its own, so a manifest
    // Phast wrote would otherwise carry quoting nobody chose.
    assert_eq!(
        output,
        indoc! {r#"
            [package]
            name = "acme/app"
            description = "some description"
        "#},
    );
}
