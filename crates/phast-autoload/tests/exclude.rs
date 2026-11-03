use std::path::{Path, PathBuf};

use phast_autoload::scanner::Exclude;

/// The package root every matcher in this file is built over.
const ROOT: &str = "/pkg";

/// A matcher over [`ROOT`] for `patterns`, which must compile to something.
fn exclude(patterns: &[&str]) -> Exclude {
    let owned: Vec<String> = patterns.iter().map(|p| (*p).to_owned()).collect();

    Exclude::new(Path::new(ROOT), &owned)
        .expect("patterns must compile")
        .expect("patterns must produce a matcher")
}

/// A path at `rel` inside the package root.
fn file(rel: &str) -> PathBuf {
    Path::new(ROOT).join(rel)
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn no_patterns_produces_no_matcher() {
    let matcher = Exclude::new(Path::new(ROOT), &[]).expect("an empty list is not an error");

    assert!(
        matcher.is_none(),
        "nothing to exclude should produce no matcher",
    );
}

#[test]
fn an_invalid_glob_is_an_error() {
    let patterns = vec!["src/[".to_owned()];

    assert!(Exclude::new(Path::new(ROOT), &patterns).is_err());
}

#[test]
fn a_pattern_naming_nothing_matches_nothing() {
    let matcher = exclude(&["/"]);

    // A bare `/` compiles to no glob at all. The list was not empty, so a matcher
    // is still built, and it excludes nothing rather than everything.
    assert!(!matcher.excludes(&file("config.php")));
    assert!(!matcher.excludes(&file("src/Any.php")));
}

#[test]
fn every_pattern_in_the_list_applies() {
    let matcher = exclude(&["/config.php", "Test/"]);

    assert!(matcher.excludes(&file("config.php")));
    assert!(matcher.excludes(&file("src/Test/Case.php")));
    assert!(!matcher.excludes(&file("src/Keep.php")));
}

// ---------------------------------------------------------------------------
// Anchoring
// ---------------------------------------------------------------------------

#[test]
fn an_anchored_pattern_matches_only_at_the_package_root() {
    let matcher = exclude(&["/Test/"]);

    assert!(matcher.excludes(&file("Test/Case.php")));
    assert!(
        !matcher.excludes(&file("src/Test/Case.php")),
        "a leading `/` anchors the pattern, so a nested `Test/` must not match",
    );
}

#[test]
fn a_floating_pattern_matches_at_any_depth() {
    let matcher = exclude(&["Test/"]);

    assert!(matcher.excludes(&file("Test/Case.php")));
    assert!(matcher.excludes(&file("src/Test/Case.php")));
}

#[test]
fn a_floating_file_name_matches_at_any_depth() {
    let matcher = exclude(&["config.php"]);

    assert!(matcher.excludes(&file("config.php")));
    assert!(matcher.excludes(&file("src/config.php")));
    assert!(
        !matcher.excludes(&file("src/myconfig.php")),
        "a floating pattern matches whole segments, not substrings of one",
    );
}

// ---------------------------------------------------------------------------
// Directories and files
// ---------------------------------------------------------------------------

#[test]
fn a_trailing_slash_excludes_everything_beneath() {
    let matcher = exclude(&["Legacy/"]);

    assert!(matcher.excludes(&file("Legacy/Old.php")));
    assert!(matcher.excludes(&file("Legacy/deep/nested/Older.php")));
}

#[test]
fn a_pattern_without_a_trailing_slash_names_one_file() {
    let matcher = exclude(&["/config.php"]);

    assert!(matcher.excludes(&file("config.php")));
    assert!(!matcher.excludes(&file("src/config.php")));
}

#[test]
fn a_trailing_slash_does_not_match_a_file_of_the_same_name() {
    let matcher = exclude(&["Legacy/"]);

    // The slash says directory, so the pattern becomes `Legacy/**` and needs
    // something beneath it. A sibling file that merely shares the name is kept.
    assert!(!matcher.excludes(&file("Legacy.php")));
    assert!(matcher.excludes(&file("Legacy/Old.php")));
}

// ---------------------------------------------------------------------------
// Wildcards
// ---------------------------------------------------------------------------

#[test]
fn a_star_matches_within_one_segment() {
    let matcher = exclude(&["src/Legacy/*"]);

    assert!(matcher.excludes(&file("src/Legacy/Old.php")));
    assert!(
        !matcher.excludes(&file("src/Legacy/deep/Old.php")),
        "`*` matches within a segment; crossing a separator is what `**` is for",
    );
}

#[test]
fn a_double_star_matches_across_segments() {
    let matcher = exclude(&["src/Legacy/**"]);

    assert!(matcher.excludes(&file("src/Legacy/Old.php")));
    assert!(matcher.excludes(&file("src/Legacy/deep/Old.php")));
    assert!(!matcher.excludes(&file("Legacy/Old.php")));
}

#[test]
fn a_star_pattern_is_still_floating() {
    let matcher = exclude(&["src/Legacy/*"]);

    // Holding `*` to one segment does not anchor the pattern: with no leading
    // `/`, the whole thing still matches wherever `src/Legacy/` appears.
    assert!(matcher.excludes(&file("deep/src/Legacy/Old.php")));
}

// ---------------------------------------------------------------------------
// How a pattern is spelled
// ---------------------------------------------------------------------------

#[test]
fn backslashes_in_a_pattern_are_read_as_separators() {
    let matcher = exclude(&["src\\Legacy\\"]);

    // A pattern written with Windows separators means the same as `src/Legacy/`.
    assert!(matcher.excludes(&file("src/Legacy/Old.php")));
    assert!(matcher.excludes(&file("src/Legacy/deep/Old.php")));
}

#[test]
fn patterns_are_case_sensitive() {
    let matcher = exclude(&["Test/"]);

    assert!(matcher.excludes(&file("Test/Case.php")));
    assert!(
        !matcher.excludes(&file("test/Case.php")),
        "matching is on the pattern, not the filesystem, so a case-insensitive \
         volume does not widen it",
    );
}

// ---------------------------------------------------------------------------
// Root scoping
// ---------------------------------------------------------------------------

#[test]
fn a_file_outside_the_package_root_is_never_excluded() {
    let matcher = exclude(&["Test/"]);

    assert!(!matcher.excludes(Path::new("/other/Test/Case.php")));
}
