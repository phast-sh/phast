use phast_version::{
    constraint::{Constraint, ConstraintError, Op, wildcard_bounds},
    parser::parse_constraint,
    stability::Stability,
    version::{NumericVersion, Version},
};

/// The version `major.minor.patch.build` at `stability` and `seq`. `precision` is
/// outside version equality, so the four here never affects an assertion.
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

/// A stable release, which is what nearly every constraint here names.
fn stable(major: u64, minor: u64, patch: u64) -> Version {
    version(major, minor, patch, 0, Stability::Stable, 0)
}

/// The constraint matching `version` and nothing else.
fn exact(version: Version) -> Constraint {
    Constraint::Exact(version)
}

/// The constraint bounding versions by `op` against `version`.
fn comparison(op: Op, version: Version) -> Constraint {
    Constraint::Comparison(op, version)
}

/// What `constraint` renders back to, which is what reaches a lock file.
fn rendered(constraint: &str) -> String {
    parse_constraint(constraint).unwrap().to_string()
}

// ---------------------------------------------------------------------------
// Wildcard
// ---------------------------------------------------------------------------

#[test]
fn parses_any_wildcard() {
    assert_eq!(parse_constraint("*").unwrap(), Constraint::Any);
}

#[test]
fn parses_major_wildcard() {
    assert_eq!(
        parse_constraint("1.*").unwrap(),
        Constraint::Wildcard {
            major: 1,
            minor: None
        },
    );
}

#[test]
fn parses_minor_wildcard() {
    assert_eq!(
        parse_constraint("1.2.*").unwrap(),
        Constraint::Wildcard {
            major: 1,
            minor: Some(2)
        },
    );
}

#[test]
fn parses_x_alias_any_wildcard() {
    assert_eq!(parse_constraint("x").unwrap(), Constraint::Any);
    assert_eq!(parse_constraint("X").unwrap(), Constraint::Any);
}

#[test]
fn parses_x_alias_major_wildcard() {
    let expected = Constraint::Wildcard {
        major: 5,
        minor: None,
    };
    assert_eq!(parse_constraint("5.x").unwrap(), expected);
    assert_eq!(parse_constraint("5.X").unwrap(), expected);
}

#[test]
fn parses_x_alias_minor_wildcard() {
    let expected = Constraint::Wildcard {
        major: 5,
        minor: Some(3),
    };
    assert_eq!(parse_constraint("5.3.x").unwrap(), expected);
    assert_eq!(parse_constraint("5.3.X").unwrap(), expected);
}

// ---------------------------------------------------------------------------
// Exact
// ---------------------------------------------------------------------------

#[test]
fn parses_exact_version() {
    assert_eq!(parse_constraint("1.2.3").unwrap(), exact(stable(1, 2, 3)));
}

#[test]
fn parses_exact_with_v_prefix() {
    assert_eq!(parse_constraint("v1.2.3").unwrap(), exact(stable(1, 2, 3)));
}

// ---------------------------------------------------------------------------
// Caret
// ---------------------------------------------------------------------------

#[test]
fn parses_caret_stable() {
    assert_eq!(
        parse_constraint("^1.2.3").unwrap(),
        Constraint::Caret(stable(1, 2, 3)),
    );
}

#[test]
fn parses_caret_minor_only() {
    assert_eq!(
        parse_constraint("^1.2").unwrap(),
        Constraint::Caret(stable(1, 2, 0)),
    );
}

#[test]
fn parses_caret_zero_minor() {
    assert_eq!(
        parse_constraint("^0.3.4").unwrap(),
        Constraint::Caret(stable(0, 3, 4)),
    );
}

#[test]
fn parses_caret_double_zero() {
    assert_eq!(
        parse_constraint("^0.0.3").unwrap(),
        Constraint::Caret(stable(0, 0, 3)),
    );
}

#[test]
fn parses_caret_with_beta_suffix() {
    assert_eq!(
        parse_constraint("^1.0.0-beta1").unwrap(),
        Constraint::Caret(version(1, 0, 0, 0, Stability::Beta, 1)),
    );
}

// ---------------------------------------------------------------------------
// Tilde
// ---------------------------------------------------------------------------

#[test]
fn parses_tilde_three_segments() {
    assert_eq!(
        parse_constraint("~1.2.3").unwrap(),
        Constraint::Tilde(stable(1, 2, 3)),
    );
}

#[test]
fn parses_tilde_two_segments() {
    assert_eq!(
        parse_constraint("~1.2").unwrap(),
        Constraint::Tilde(stable(1, 2, 0)),
    );
}

#[test]
fn parses_tilde_one_segment() {
    assert_eq!(
        parse_constraint("~1").unwrap(),
        Constraint::Tilde(stable(1, 0, 0)),
    );
}

// ---------------------------------------------------------------------------
// Comparison operators
// ---------------------------------------------------------------------------

#[test]
fn parses_gte() {
    assert_eq!(
        parse_constraint(">=1.0").unwrap(),
        comparison(Op::Gte, stable(1, 0, 0)),
    );
}

#[test]
fn parses_lte() {
    assert_eq!(
        parse_constraint("<=2.0").unwrap(),
        comparison(Op::Lte, stable(2, 0, 0)),
    );
}

#[test]
fn parses_gt() {
    assert_eq!(
        parse_constraint(">1.0").unwrap(),
        comparison(Op::Gt, stable(1, 0, 0)),
    );
}

#[test]
fn parses_lt() {
    assert_eq!(
        parse_constraint("<2.0").unwrap(),
        comparison(Op::Lt, stable(2, 0, 0)),
    );
}

#[test]
fn parses_ne() {
    assert_eq!(
        parse_constraint("!=1.5").unwrap(),
        comparison(Op::Ne, stable(1, 5, 0)),
    );
}

// ---------------------------------------------------------------------------
// Comparison with space between operator and version
// ---------------------------------------------------------------------------

#[test]
fn parses_gte_with_space() {
    assert_eq!(
        parse_constraint(">= 1.0").unwrap(),
        comparison(Op::Gte, stable(1, 0, 0)),
    );
}

#[test]
fn parses_lt_with_space() {
    assert_eq!(
        parse_constraint("< 2.0").unwrap(),
        comparison(Op::Lt, stable(2, 0, 0)),
    );
}

// ---------------------------------------------------------------------------
// AND (comma and whitespace)
// ---------------------------------------------------------------------------

#[test]
fn parses_and_comma() {
    assert_eq!(
        parse_constraint(">=1.0,<2.0").unwrap(),
        Constraint::And(vec![
            comparison(Op::Gte, stable(1, 0, 0)),
            comparison(Op::Lt, stable(2, 0, 0)),
        ]),
    );
}

#[test]
fn parses_and_space() {
    assert_eq!(
        parse_constraint(">=1.0 <2.0").unwrap(),
        Constraint::And(vec![
            comparison(Op::Gte, stable(1, 0, 0)),
            comparison(Op::Lt, stable(2, 0, 0)),
        ]),
    );
}

#[test]
fn parses_and_with_operator_spaces() {
    assert_eq!(
        parse_constraint(">= 1.0 < 2.0").unwrap(),
        Constraint::And(vec![
            comparison(Op::Gte, stable(1, 0, 0)),
            comparison(Op::Lt, stable(2, 0, 0)),
        ]),
    );
}

#[test]
fn parses_and_comma_with_spaces() {
    assert_eq!(
        parse_constraint(">=1.0 , <2.0").unwrap(),
        Constraint::And(vec![
            comparison(Op::Gte, stable(1, 0, 0)),
            comparison(Op::Lt, stable(2, 0, 0)),
        ]),
    );
}

#[test]
fn parses_and_three_terms() {
    assert_eq!(
        parse_constraint(">=1.0,<2.0,!=1.5").unwrap(),
        Constraint::And(vec![
            comparison(Op::Gte, stable(1, 0, 0)),
            comparison(Op::Lt, stable(2, 0, 0)),
            comparison(Op::Ne, stable(1, 5, 0)),
        ]),
    );
}

// ---------------------------------------------------------------------------
// OR
// ---------------------------------------------------------------------------

#[test]
fn parses_or_two_exact() {
    assert_eq!(
        parse_constraint("1.0||2.0").unwrap(),
        Constraint::Or(vec![exact(stable(1, 0, 0)), exact(stable(2, 0, 0)),]),
    );
}

#[test]
fn parses_or_with_spaces() {
    assert_eq!(
        parse_constraint("1.0 || 2.0").unwrap(),
        Constraint::Or(vec![exact(stable(1, 0, 0)), exact(stable(2, 0, 0)),]),
    );
}

#[test]
fn parses_or_three_versions() {
    assert_eq!(
        parse_constraint("1.0 || 2.0 || 3.0").unwrap(),
        Constraint::Or(vec![
            exact(stable(1, 0, 0)),
            exact(stable(2, 0, 0)),
            exact(stable(3, 0, 0)),
        ]),
    );
}

#[test]
fn parses_or_with_ranges() {
    assert_eq!(
        parse_constraint(">=1.0 <1.5 || >=2.0 <2.5").unwrap(),
        Constraint::Or(vec![
            Constraint::And(vec![
                comparison(Op::Gte, stable(1, 0, 0)),
                comparison(Op::Lt, stable(1, 5, 0)),
            ]),
            Constraint::And(vec![
                comparison(Op::Gte, stable(2, 0, 0)),
                comparison(Op::Lt, stable(2, 5, 0)),
            ]),
        ]),
    );
}

// ---------------------------------------------------------------------------
// Branch constraint
// ---------------------------------------------------------------------------

#[test]
fn parses_branch_dev_main() {
    assert_eq!(
        parse_constraint("dev-main").unwrap(),
        Constraint::Branch("main".to_string()),
    );
}

#[test]
fn parses_branch_dev_feature() {
    assert_eq!(
        parse_constraint("dev-feature/foo").unwrap(),
        Constraint::Branch("feature/foo".to_string()),
    );
}

#[test]
fn branch_display_roundtrip() {
    assert_eq!(
        parse_constraint("dev-main").unwrap().to_string(),
        "dev-main",
    );
    assert_eq!(
        parse_constraint("dev-feature/foo").unwrap().to_string(),
        "dev-feature/foo",
    );
}

// ---------------------------------------------------------------------------
// Hyphen range
// ---------------------------------------------------------------------------

#[test]
fn parses_hyphen_range_no_patch_exclusive_upper() {
    // Right side has no patch segment: upper bound is exclusive at next minor.
    assert_eq!(
        parse_constraint("1.0 - 2.0").unwrap(),
        Constraint::And(vec![
            comparison(Op::Gte, stable(1, 0, 0)),
            comparison(Op::Lt, version(2, 1, 0, 0, Stability::Dev, 0)),
        ]),
    );
}

#[test]
fn parses_hyphen_range_with_patch_inclusive_upper() {
    // Right side has a patch segment: upper bound is inclusive.
    assert_eq!(
        parse_constraint("1.0.0 - 2.1.0").unwrap(),
        Constraint::And(vec![
            comparison(Op::Gte, stable(1, 0, 0)),
            comparison(Op::Lte, stable(2, 1, 0)),
        ]),
    );
}

#[test]
fn parses_hyphen_range_counting_only_numeric_segments() {
    // "2.0-beta.1" has two numeric segments despite three raw dots, and it is the
    // segment count that decides the bound, so the upper bound is exclusive at the
    // next minor rather than inclusive. Precision itself is outside version
    // equality, so the bound is the only place the count shows up.
    assert_eq!(
        parse_constraint("1.0 - 2.0-beta.1").unwrap(),
        Constraint::And(vec![
            comparison(Op::Gte, stable(1, 0, 0)),
            comparison(Op::Lt, version(2, 1, 0, 0, Stability::Dev, 0)),
        ]),
    );
}

// ---------------------------------------------------------------------------
// Stability modifier (@stability)
// ---------------------------------------------------------------------------

#[test]
fn parses_exact_with_stability_modifier() {
    // @stability is stripped; version comparison is unaffected
    assert_eq!(
        parse_constraint("1.0@beta").unwrap(),
        exact(stable(1, 0, 0)),
    );
}

#[test]
fn parses_comparison_with_stability_modifier() {
    assert_eq!(
        parse_constraint(">=1.0@stable").unwrap(),
        comparison(Op::Gte, stable(1, 0, 0)),
    );
}

// ---------------------------------------------------------------------------
// lower_bound
// ---------------------------------------------------------------------------

/// The floor of a constraint string, as a `major.minor.patch` triple.
fn floor(constraint: &str) -> Option<(u64, u64, u64)> {
    let Version::Numeric(v) = parse_constraint(constraint).unwrap().lower_bound()? else {
        return None;
    };
    Some((v.segments[0], v.segments[1], v.segments[2]))
}

#[test]
fn lower_bound_of_simple_bounds() {
    assert_eq!(floor(">=8.1"), Some((8, 1, 0)));
    assert_eq!(floor("^8.1"), Some((8, 1, 0)));
    assert_eq!(floor("~8.1.2"), Some((8, 1, 2)));
    assert_eq!(floor("8.2.*"), Some((8, 2, 0)));
    assert_eq!(floor("8.1.5"), Some((8, 1, 5)));
}

#[test]
fn lower_bound_of_upper_only_is_none() {
    assert_eq!(floor("<9.0"), None);
}

#[test]
fn lower_bound_of_any_is_none() {
    assert_eq!(floor("*"), None);
}

#[test]
fn lower_bound_of_range_is_the_max() {
    // `>=8.1 <9.0`: the `<9.0` term has no floor, so the floor is 8.1.
    assert_eq!(floor(">=8.1 <9.0"), Some((8, 1, 0)));
}

#[test]
fn lower_bound_of_union_is_the_min() {
    // `^7.2 || ^8.0`: satisfying either branch suffices, so the floor is 7.2.
    assert_eq!(floor("^7.2 || ^8.0"), Some((7, 2, 0)));
}

#[test]
fn lower_bound_of_floorless_union_is_none() {
    // `<5.0 || >=8.0`: the `<5.0` branch admits arbitrarily low versions.
    assert_eq!(floor("<5.0 || >=8.0"), None);
}

// ---------------------------------------------------------------------------
// The slot a wildcard spans
// ---------------------------------------------------------------------------

#[test]
fn wildcard_bounds_span_the_segment_left_open() {
    // Half-open, and both ends carry Dev stability so a pre-release inside the
    // slot matches: `1.*` has to admit `1.0.0-beta1`.
    let (lower, upper) = wildcard_bounds(1, None);
    assert_eq!(lower.to_string(), "1.0-dev");
    assert_eq!(upper.to_string(), "2.0-dev");

    let (lower, upper) = wildcard_bounds(1, Some(2));
    assert_eq!(lower.to_string(), "1.2-dev");
    assert_eq!(upper.to_string(), "1.3-dev");
}

#[test]
fn wildcard_bounds_on_a_zero_major_stay_inside_it() {
    let (lower, upper) = wildcard_bounds(0, Some(0));
    assert_eq!(lower.to_string(), "0.0-dev");
    assert_eq!(upper.to_string(), "0.1-dev");
}

// ---------------------------------------------------------------------------
// Rendering a constraint back to a string
// ---------------------------------------------------------------------------

#[test]
fn every_shape_renders_in_composer_syntax() {
    // What a lock file records for a path or git package's requirements is this
    // rendering, so a wrong arm here writes a constraint nobody asked for.
    assert_eq!(rendered("*"), "*");
    assert_eq!(rendered("1.2.3"), "1.2.3");
    assert_eq!(rendered("^1.2.3"), "^1.2.3");
    assert_eq!(rendered("~1.2.3"), "~1.2.3");
    assert_eq!(rendered("1.*"), "1.*");
    assert_eq!(rendered("1.2.*"), "1.2.*");
    assert_eq!(rendered("dev-main"), "dev-main");
    assert_eq!(rendered("dev-feature/foo"), "dev-feature/foo");
    assert_eq!(rendered(">=1.0 <2.0"), ">=1.0 <2.0");
    assert_eq!(rendered("1.0 || 2.0"), "1.0 || 2.0");
    let union = ">=1.0 <1.5 || >=2.0 <2.5";
    assert_eq!(rendered(union), union);
}

#[test]
fn every_operator_renders_as_it_was_written() {
    assert_eq!(rendered(">1.0"), ">1.0");
    assert_eq!(rendered(">=1.0"), ">=1.0");
    assert_eq!(rendered("<2.0"), "<2.0");
    assert_eq!(rendered("<=2.0"), "<=2.0");
    assert_eq!(rendered("!=1.5"), "!=1.5");
}

#[test]
fn rendering_settles_on_one_spelling_of_what_was_accepted() {
    // The parser takes several spellings of the same constraint and the rendering
    // picks one, so a manifest's formatting does not travel into the lock.
    assert_eq!(rendered("v1.2.3"), "1.2.3");
    assert_eq!(rendered("5.x"), "5.*");
    assert_eq!(rendered(">= 1.0"), ">=1.0");
    assert_eq!(rendered(">=1.0,<2.0"), ">=1.0 <2.0");
    assert_eq!(rendered("~1"), "~1.0");
    // The stability modifier is dropped at parse time and never comes back.
    assert_eq!(rendered("1.0@beta"), "1.0");
}

#[test]
fn a_rendered_constraint_parses_back_to_the_same_constraint() {
    // The lock is read as well as written, so a rendering that parses to
    // something else would change what a package requires on the way through.
    for raw in [
        "*",
        "1.2.3",
        "^1.2.3",
        "~1.2",
        "1.*",
        "1.2.*",
        ">=1.0",
        "!=1.5",
        "dev-main",
        ">=1.0 <2.0",
        "1.0 || 2.0",
    ] {
        let one = parse_constraint(raw).unwrap();
        let two = parse_constraint(&one.to_string()).unwrap();
        assert_eq!(one, two, "for {raw}");
    }
}

#[test]
fn a_hyphen_range_renders_as_the_pair_of_bounds_it_expanded_to() {
    // There is no hyphen in the parsed form, so the rendering shows the bounds.
    // The exclusive upper carries Dev stability and renders with a `-dev` suffix,
    // which a reader of the lock will see rather than the range that was written.
    assert_eq!(rendered("1.0 - 2.0"), ">=1.0 <2.1-dev");
    assert_eq!(rendered("1.0.0 - 2.1.0"), ">=1.0 <=2.1");
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn rejects_empty_constraint() {
    assert!(matches!(
        parse_constraint("").unwrap_err(),
        ConstraintError::Empty
    ));
}

#[test]
fn rejects_whitespace_only_constraint() {
    assert!(matches!(
        parse_constraint("   ").unwrap_err(),
        ConstraintError::Empty
    ));
}

#[test]
fn rejects_invalid_version_in_caret() {
    let raw = "^not-a-version-really";
    let error = parse_constraint(raw).unwrap_err();
    assert!(matches!(error, ConstraintError::Invalid(_, _)), "{error}");
    assert!(error.to_string().contains(raw), "{error}");
}

#[test]
fn rejects_an_operator_with_no_version_after_it() {
    // A truncated constraint is a manifest typo, and the message has to quote it
    // back rather than report an empty version nobody wrote.
    for raw in [">=", "<", "~", "^"] {
        let error = parse_constraint(raw).unwrap_err();
        let invalid = matches!(error, ConstraintError::Invalid(_, _));
        assert!(invalid, "{raw}: {error}");
    }
}

#[test]
fn rejects_a_union_missing_one_of_its_sides() {
    // A trailing or leading `||` leaves a branch with nothing in it, which reads
    // as an empty constraint rather than an invalid one.
    for raw in ["1.0 ||", "|| 1.0"] {
        let error = parse_constraint(raw).unwrap_err();
        assert!(matches!(error, ConstraintError::Empty), "{raw}: {error}");
    }
}
