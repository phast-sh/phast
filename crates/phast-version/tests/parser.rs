use phast_version::{
    Constraint,
    parser::{parse_branch_alias, parse_constraint, parse_declared_version, parse_version},
    stability::Stability,
    version::{NumericVersion, Version},
};

use std::str::FromStr;

/// The version `major.minor.patch.build` at `stability` and `seq`. `precision` is
/// outside version equality, so the four here never decides an assertion.
fn version(
    major: u64,
    minor: u64,
    patch: u64,
    build: u64,
    stability: Stability,
    seq: u32,
) -> Version {
    Version::Numeric(NumericVersion {
        segments: [major, minor, patch, build],
        stability,
        stability_seq: seq,
        precision: 4,
    })
}

/// A stable release, which is what most of these assertions are about.
fn stable(major: u64, minor: u64, patch: u64) -> Version {
    version(major, minor, patch, 0, Stability::Stable, 0)
}

/// The branch `name`, as `dev-{name}` parses to.
fn branch(name: &str) -> Version {
    Version::Branch(name.to_owned())
}

// ---------------------------------------------------------------------------
// Standard numeric versions
// ---------------------------------------------------------------------------

#[test]
fn parses_simple_version() {
    assert_eq!(parse_version("1.2.3").unwrap(), stable(1, 2, 3));
}

#[test]
fn parses_v_prefix_lowercase() {
    assert_eq!(parse_version("v1.2.3").unwrap(), stable(1, 2, 3));
}

#[test]
fn parses_v_prefix_uppercase() {
    assert_eq!(parse_version("V1.2.3").unwrap(), stable(1, 2, 3));
}

#[test]
fn parses_two_segment_version() {
    assert_eq!(parse_version("1.2").unwrap(), stable(1, 2, 0));
}

#[test]
fn parses_one_segment_version() {
    assert_eq!(parse_version("1").unwrap(), stable(1, 0, 0));
}

#[test]
fn parses_four_segment_version() {
    assert_eq!(
        parse_version("1.2.3.4").unwrap(),
        version(1, 2, 3, 4, Stability::Stable, 0),
    );
}

#[test]
fn trims_surrounding_whitespace() {
    assert_eq!(parse_version("  1.2.3  ").unwrap(), stable(1, 2, 3));
}

#[test]
fn parses_a_date_as_one_large_major() {
    // A PHP extension often reports its version as a date integer: ext-dom says
    // 20031129. It is a single segment, so it lands in the major and compares
    // above every ordinary version, which is what lets `ext-dom: *` resolve.
    assert_eq!(parse_version("20031129").unwrap(), stable(20031129, 0, 0));
}

// ---------------------------------------------------------------------------
// Input the parser refuses
// ---------------------------------------------------------------------------

#[test]
fn rejects_empty_string() {
    assert!(parse_version("").is_err());
}

#[test]
fn rejects_whitespace_only() {
    assert!(parse_version("   ").is_err());
}

#[test]
fn rejects_five_segments() {
    assert!(parse_version("1.2.3.4.5").is_err());
}

#[test]
fn rejects_a_segment_that_is_not_a_number() {
    // An empty segment, a sign, or embedded whitespace: each names the segment it
    // choked on, so a long constraint says which part of it is wrong.
    for raw in ["1..2", "1.", ".1", "1.2.", "-1.0", "1.-2", "v", "1 2"] {
        let error = parse_version(raw).unwrap_err().to_string();
        assert!(error.contains("expected a number"), "{raw}: {error}");
    }
}

#[test]
fn rejects_a_segment_too_large_to_hold() {
    // Refused rather than wrapped or saturated, which would silently reorder it
    // against every other version.
    assert!(parse_version("99999999999999999999999.0").is_err());
}

// ---------------------------------------------------------------------------
// Stability suffixes
// ---------------------------------------------------------------------------

#[test]
fn parses_hyphen_beta_with_seq() {
    assert_eq!(
        parse_version("1.0.0-beta1").unwrap(),
        version(1, 0, 0, 0, Stability::Beta, 1),
    );
}

#[test]
fn parses_hyphen_beta_dot_seq() {
    assert_eq!(
        parse_version("1.0.0-beta.1").unwrap(),
        version(1, 0, 0, 0, Stability::Beta, 1),
    );
}

#[test]
fn parses_inline_beta_no_hyphen() {
    assert_eq!(
        parse_version("1.0.0beta1").unwrap(),
        version(1, 0, 0, 0, Stability::Beta, 1),
    );
}

#[test]
fn parses_rc_with_seq() {
    assert_eq!(
        parse_version("1.0.0-RC2").unwrap(),
        version(1, 0, 0, 0, Stability::RC, 2),
    );
}

#[test]
fn parses_alpha_no_seq() {
    assert_eq!(
        parse_version("1.0.0-alpha").unwrap(),
        version(1, 0, 0, 0, Stability::Alpha, 0),
    );
}

#[test]
fn parses_patch_aliases_to_stable() {
    assert_eq!(
        parse_version("1.0.0-patch").unwrap(),
        version(1, 0, 0, 0, Stability::Stable, 0),
    );
}

#[test]
fn parses_pl_aliases_to_stable_with_seq() {
    assert_eq!(
        parse_version("1.0.0-pl1").unwrap(),
        version(1, 0, 0, 0, Stability::Stable, 1),
    );
}

// ---------------------------------------------------------------------------
// Branch versions
// ---------------------------------------------------------------------------

#[test]
fn parses_dev_main() {
    assert_eq!(parse_version("dev-main").unwrap(), branch("main"));
}

#[test]
fn parses_dev_master() {
    assert_eq!(parse_version("dev-master").unwrap(), branch("master"));
}

#[test]
fn parses_branch_with_slash() {
    assert_eq!(
        parse_version("dev-feature/foo").unwrap(),
        branch("feature/foo"),
    );
}

#[test]
fn parses_maintenance_branch() {
    assert_eq!(parse_version("dev-1.x").unwrap(), branch("1.x"));
}

#[test]
fn parses_legacy_dev_suffix() {
    assert_eq!(parse_version("feature-dev").unwrap(), branch("feature"));
}

#[test]
fn parses_1_0_0_dev_as_branch() {
    // All `{rest}-dev` strings are branches, including those where rest looks like a version number.
    assert_eq!(parse_version("1.0.0-dev").unwrap(), branch("1.0.0"));
}

#[test]
fn parses_9999999_dev_as_branch() {
    // Composer's internal bleeding-edge alias.
    assert_eq!(parse_version("9999999-dev").unwrap(), branch("9999999"));
}

// ---------------------------------------------------------------------------
// Branch aliases (extra.branch-alias targets)
// ---------------------------------------------------------------------------

const SENTINEL: u64 = u64::MAX >> 1;

/// A dev-stability numeric version. `precision` is not part of equality, so it is
/// fixed here; the alias parser sets it from the segment count.
fn dev(major: u64, minor: u64, patch: u64, build: u64) -> NumericVersion {
    NumericVersion {
        segments: [major, minor, patch, build],
        stability: Stability::Dev,
        stability_seq: 0,
        precision: 4,
    }
}

#[test]
fn branch_alias_expands_x_placeholder_to_sentinel() {
    // `1.2.x-dev` sits at the top of the 1.2 series, at dev stability.
    assert_eq!(
        parse_branch_alias("1.2.x-dev").unwrap(),
        dev(1, 2, SENTINEL, 0),
    );
}

#[test]
fn branch_alias_accepts_two_segment_and_capital_x() {
    assert_eq!(
        parse_branch_alias("2.x-dev").unwrap(),
        dev(2, SENTINEL, 0, 0),
    );
    assert_eq!(
        parse_branch_alias("1.0.X-dev").unwrap(),
        dev(1, 0, SENTINEL, 0),
    );
}

#[test]
fn branch_alias_without_placeholder_is_top_of_series() {
    // `1.2-dev` (no explicit `x`) means the top of the 1.2 line: `1.2.x-dev`.
    assert_eq!(
        parse_branch_alias("1.2-dev").unwrap(),
        dev(1, 2, SENTINEL, 0),
    );
    // So it satisfies a numeric `^1.2`, unlike a plain `1.2.0-dev` would.
    let alias = Version::Numeric(parse_branch_alias("1.2-dev").unwrap());
    assert!(Constraint::from_str("^1.2").unwrap().matches(&alias));
}

#[test]
fn branch_alias_matches_caret_range_for_its_series() {
    // The whole point: `1.2.x-dev` must satisfy `^1.2` so a branch can fill a
    // numeric require. Range containment is what the resolver relies on.
    let alias = Version::Numeric(parse_branch_alias("1.2.x-dev").unwrap());
    let constraint = Constraint::from_str("^1.2").unwrap();
    assert!(constraint.matches(&alias), "{alias} should satisfy ^1.2");
}

#[test]
fn branch_alias_rejects_non_dev_and_malformed_targets() {
    // No `-dev` suffix, a branch name, or bare `-dev`: unrecognized.
    assert!(parse_branch_alias("1.2.x").is_err());
    assert!(parse_branch_alias("dev-main").is_err());
    assert!(parse_branch_alias("-dev").is_err());
    // Too many segments.
    assert!(parse_branch_alias("1.2.3.4.5-dev").is_err());
    // Non-numeric, non-placeholder segment.
    assert!(parse_branch_alias("1.foo-dev").is_err());
}

// ---------------------------------------------------------------------------
// Ordering: numeric segments
// ---------------------------------------------------------------------------

#[test]
fn higher_major_wins() {
    assert!(stable(2, 0, 0) > stable(1, 9, 9));
}

#[test]
fn higher_minor_wins_same_major() {
    assert!(stable(1, 3, 0) > stable(1, 2, 9));
}

#[test]
fn higher_patch_wins_same_major_minor() {
    assert!(stable(1, 2, 3) > stable(1, 2, 2));
}

#[test]
fn equal_versions_are_equal() {
    assert_eq!(stable(1, 2, 3), stable(1, 2, 3));
}

// ---------------------------------------------------------------------------
// Ordering: stability within same numeric version
// ---------------------------------------------------------------------------

#[test]
fn stable_beats_rc() {
    assert!(version(1, 0, 0, 0, Stability::Stable, 0) > version(1, 0, 0, 0, Stability::RC, 99));
}

#[test]
fn rc_beats_beta() {
    assert!(version(1, 0, 0, 0, Stability::RC, 1) > version(1, 0, 0, 0, Stability::Beta, 99));
}

#[test]
fn beta_beats_alpha() {
    assert!(version(1, 0, 0, 0, Stability::Beta, 1) > version(1, 0, 0, 0, Stability::Alpha, 99));
}

#[test]
fn alpha_beats_dev() {
    assert!(version(1, 0, 0, 0, Stability::Alpha, 1) > version(1, 0, 0, 0, Stability::Dev, 99));
}

#[test]
fn higher_seq_wins_same_stability() {
    assert!(version(1, 0, 0, 0, Stability::Beta, 2) > version(1, 0, 0, 0, Stability::Beta, 1));
}

// ---------------------------------------------------------------------------
// Ordering: numeric vs branch
// ---------------------------------------------------------------------------

#[test]
fn any_numeric_sorts_before_any_branch() {
    assert!(stable(999, 999, 999) < branch("main"));
}

#[test]
fn branches_sort_by_name() {
    assert!(branch("abc") < branch("xyz"));
    assert!(branch("main") < branch("master"));
    // Byte order rather than a case-insensitive collation, so an upper-case name
    // sorts before every lower-case one.
    assert!(branch("Main") < branch("main"));
}

// ---------------------------------------------------------------------------
// Rendering a version back to a string
// ---------------------------------------------------------------------------

#[test]
fn display_omits_trailing_zero_segments() {
    assert_eq!(stable(1, 2, 0).to_string(), "1.2");
    assert_eq!(stable(1, 0, 0).to_string(), "1.0");
}

#[test]
fn display_keeps_non_zero_patch() {
    assert_eq!(stable(1, 2, 3).to_string(), "1.2.3");
}

#[test]
fn display_stability_suffix() {
    assert_eq!(
        version(1, 0, 0, 0, Stability::Beta, 1).to_string(),
        "1.0-beta1",
    );
    assert_eq!(version(1, 0, 0, 0, Stability::RC, 2).to_string(), "1.0-RC2");
    assert_eq!(
        version(1, 0, 0, 0, Stability::Alpha, 0).to_string(),
        "1.0-alpha",
    );
}

#[test]
fn display_branch() {
    assert_eq!(branch("main").to_string(), "dev-main");
    assert_eq!(branch("feature/foo").to_string(), "dev-feature/foo");
}

#[test]
fn a_rendered_version_parses_back_to_the_same_version() {
    // A lock records versions as strings and reads them back, so a rendering the
    // parser cannot recover would change what is installed on the way through.
    // The string need not match the input: `1` renders `1.0`, which is the same
    // version, since trailing zeros carry no meaning.
    for raw in [
        "1.2.3",
        "1.2",
        "1",
        "1.2.3.4",
        "1.0.0-beta1",
        "1.0.0-RC2",
        "1.0.0-alpha",
        "20031129",
        "dev-main",
        "dev-feature/foo",
    ] {
        let one = parse_version(raw).unwrap();
        let two = parse_version(&one.to_string()).unwrap();
        assert_eq!(one, two, "for {raw}");
    }
}

// ---------------------------------------------------------------------------
// Declared version pins (options.versions / inline package version)
// ---------------------------------------------------------------------------

#[test]
fn declared_branch_alias_is_a_dev_numeric_satisfying_a_caret() {
    // `3.7.x-dev` is a branch alias, not a branch: a top-of-series dev version.
    let version = parse_declared_version("3.7.x-dev").unwrap();
    assert!(matches!(version, Version::Numeric(ref n) if n.stability == Stability::Dev));

    // It renders back with the `x` placeholder, not the sort sentinel.
    assert_eq!(version.to_string(), "3.7.x-dev");

    // And it satisfies a numeric `^3.7`, which is the whole point.
    assert!(parse_constraint("^3.7").unwrap().matches(&version));
    assert!(!parse_constraint("^4.0").unwrap().matches(&version));
}

#[test]
fn a_declared_dev_target_is_an_alias_where_a_read_version_is_a_branch() {
    // The same string means different things in the two positions. Read off a
    // registry, `1.2-dev` is the branch named `1.2`. Declared as a pin, it is the
    // top of the 1.2 series, so it can satisfy a numeric require.
    for raw in ["1.2-dev", "1.0.0-dev", "9999999-dev"] {
        let read = parse_version(raw).unwrap();
        assert!(matches!(read, Version::Branch(_)), "{raw}: {read}");

        let declared = parse_declared_version(raw).unwrap();
        assert!(matches!(declared, Version::Numeric(_)), "{raw}: {declared}");
    }

    assert_eq!(
        parse_declared_version("1.2-dev").unwrap().to_string(),
        "1.2.x-dev",
    );
}

#[test]
fn a_declared_version_that_is_no_alias_falls_back_to_reading_it() {
    // `feature-dev` is not a series, so the alias parser declines and the ordinary
    // reading stands: a branch called `feature`.
    assert_eq!(
        parse_declared_version("feature-dev").unwrap(),
        branch("feature"),
    );
}

#[test]
fn a_declared_version_that_is_neither_is_refused() {
    assert!(parse_declared_version("not a version").is_err());
}

#[test]
fn declared_concrete_and_branch_forms_are_unchanged() {
    // A concrete version stays numeric.
    assert_eq!(
        parse_declared_version("1.4.2").unwrap().to_string(),
        "1.4.2",
    );
    // A real `dev-<branch>` stays a branch.
    assert_eq!(
        parse_declared_version("dev-main").unwrap(),
        Version::Branch("main".to_owned()),
    );
}
