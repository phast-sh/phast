use phast_resolver::ranges::{constraint_to_range, range_to_version};
use phast_version::{
    constraint::Constraint,
    parser::parse_constraint,
    stability::Stability,
    version::{NumericVersion, Version},
};

use pubgrub::Ranges;

/// The version `major.minor.patch` at `stability` and `seq`, three segments precise.
fn version(major: u64, minor: u64, patch: u64, stability: Stability, seq: u32) -> Version {
    Version::Numeric(NumericVersion {
        segments: [major, minor, patch, 0],
        stability,
        stability_seq: seq,
        precision: 3,
    })
}

/// A stable release, the shape nearly every assertion here is about.
fn stable(major: u64, minor: u64, patch: u64) -> Version {
    version(major, minor, patch, Stability::Stable, 0)
}

/// A dev-stability version at the same numbers, which sorts below the stable one.
fn dev(major: u64, minor: u64, patch: u64) -> Version {
    version(major, minor, patch, Stability::Dev, 0)
}

/// The interval `constraint` translates to, parsed from the syntax a user writes.
fn range(constraint: &str) -> Ranges<Version> {
    constraint_to_range(&parse_constraint(constraint).unwrap())
}

/// Whether `ranges` admits `version`.
fn contains(ranges: &Ranges<Version>, version: Version) -> bool {
    ranges.contains(&version)
}

// ---------------------------------------------------------------------------
// A wildcard spans the slot it leaves open
// ---------------------------------------------------------------------------

#[test]
fn a_bare_star_admits_everything() {
    assert_eq!(range("*"), Ranges::full());
}

#[test]
fn a_minor_wildcard_admits_the_whole_major() {
    let r = range("1.*");
    assert!(contains(&r, stable(1, 0, 0)));
    assert!(contains(&r, stable(1, 5, 3)));
    assert!(contains(&r, stable(1, 9, 9)));
}

#[test]
fn a_minor_wildcard_stops_at_the_next_major() {
    let r = range("1.*");
    assert!(!contains(&r, stable(2, 0, 0)));
    assert!(!contains(&r, stable(0, 9, 9)));
}

#[test]
fn a_patch_wildcard_admits_the_whole_minor() {
    let r = range("1.2.*");
    assert!(contains(&r, stable(1, 2, 0)));
    assert!(contains(&r, stable(1, 2, 9)));
}

#[test]
fn a_patch_wildcard_stops_at_the_next_minor() {
    let r = range("1.2.*");
    assert!(!contains(&r, stable(1, 3, 0)));
    assert!(!contains(&r, stable(1, 1, 9)));
}

// ---------------------------------------------------------------------------
// An exact version is a single point
// ---------------------------------------------------------------------------

#[test]
fn an_exact_version_admits_only_itself() {
    let r = range("1.2.3");
    assert!(contains(&r, stable(1, 2, 3)));
    assert!(!contains(&r, stable(1, 2, 4)));
    assert!(!contains(&r, stable(1, 2, 2)));
}

// ---------------------------------------------------------------------------
// Which side of a comparison the bound falls on
// ---------------------------------------------------------------------------

#[test]
fn gte_admits_its_own_version() {
    let r = range(">=1.0");
    assert!(contains(&r, stable(1, 0, 0)));
    assert!(contains(&r, stable(2, 0, 0)));
    assert!(!contains(&r, stable(0, 9, 9)));
}

#[test]
fn gt_excludes_its_own_version() {
    let r = range(">1.0");
    assert!(!contains(&r, stable(1, 0, 0)));
    assert!(contains(&r, stable(1, 0, 1)));
}

#[test]
fn lte_admits_its_own_version() {
    let r = range("<=2.0");
    assert!(contains(&r, stable(2, 0, 0)));
    assert!(contains(&r, stable(1, 9, 9)));
    assert!(!contains(&r, stable(2, 0, 1)));
}

#[test]
fn lt_excludes_its_own_version() {
    let r = range("<2.0");
    assert!(!contains(&r, stable(2, 0, 0)));
    assert!(contains(&r, stable(1, 9, 9)));
}

#[test]
fn ne_cuts_one_point_out_of_everything() {
    let r = range("!=1.5");
    assert!(contains(&r, stable(1, 4, 9)));
    assert!(!contains(&r, stable(1, 5, 0)));
    assert!(contains(&r, stable(1, 5, 1)));
    assert!(contains(&r, stable(2, 0, 0)));
}

// ---------------------------------------------------------------------------
// How far a caret reaches
// ---------------------------------------------------------------------------

#[test]
fn a_caret_on_a_nonzero_major_reaches_the_next_major() {
    let r = range("^1.2.3");
    // The lower bound is inclusive and the upper exclusive, which is the whole
    // shape of a caret.
    assert!(contains(&r, stable(1, 2, 3)));
    assert!(contains(&r, stable(1, 9, 9)));
    assert!(!contains(&r, stable(2, 0, 0)));
    assert!(!contains(&r, stable(1, 2, 2)));
}

#[test]
fn a_caret_on_a_zero_major_reaches_only_the_next_minor() {
    let r = range("^0.3.4");
    assert!(contains(&r, stable(0, 3, 4)));
    assert!(contains(&r, stable(0, 3, 9)));
    assert!(!contains(&r, stable(0, 4, 0)));
    assert!(!contains(&r, stable(1, 0, 0)));
}

#[test]
fn a_caret_on_a_zero_minor_reaches_only_the_next_patch() {
    let r = range("^0.0.3");
    assert!(contains(&r, stable(0, 0, 3)));
    assert!(!contains(&r, stable(0, 0, 4)));
    assert!(!contains(&r, stable(0, 1, 0)));
}

#[test]
fn a_caret_on_a_zero_minor_with_no_patch_reaches_the_next_minor() {
    // ^0.0 (patch omitted) must give [0.0.0, 0.1.0) per Composer spec.
    let r = range("^0.0");
    assert!(contains(&r, stable(0, 0, 0)));
    assert!(contains(&r, stable(0, 0, 9)));
    assert!(!contains(&r, stable(0, 1, 0)));
}

#[test]
fn a_caret_admits_the_pre_release_it_names_but_nothing_below_it() {
    let r = range("^1.0.0-beta1");
    assert!(contains(&r, version(1, 0, 0, Stability::Beta, 1)));
    assert!(contains(&r, stable(1, 5, 0)));
    assert!(!contains(&r, stable(2, 0, 0)));
    assert!(!contains(&r, version(1, 0, 0, Stability::Alpha, 1)));
}

#[test]
fn a_caret_on_a_branch_admits_only_that_branch() {
    // No interval makes sense around a branch, so the constraint collapses to the
    // one version it names rather than spanning anything.
    let main = Version::Branch("main".to_owned());
    let r = constraint_to_range(&Constraint::Caret(main.clone()));

    assert!(contains(&r, main));
    assert!(!contains(&r, stable(1, 0, 0)));
}

// ---------------------------------------------------------------------------
// How far a tilde reaches
// ---------------------------------------------------------------------------

#[test]
fn a_tilde_with_a_patch_reaches_the_next_minor() {
    let r = range("~1.2.3");
    assert!(contains(&r, stable(1, 2, 3)));
    assert!(contains(&r, stable(1, 2, 9)));
    assert!(!contains(&r, stable(1, 3, 0)));
}

#[test]
fn a_tilde_with_a_zero_patch_reaches_the_next_minor_too() {
    // ~1.2.0 must behave like ~1.2.3: three segments present, lock to minor.
    // Composer spec: ~X.Y.Z always locks to next minor regardless of Z's value.
    let r = range("~1.2.0");
    assert!(contains(&r, stable(1, 2, 0)));
    assert!(contains(&r, stable(1, 2, 9)));
    assert!(!contains(&r, stable(1, 3, 0)));
    assert!(!contains(&r, stable(2, 0, 0)));
}

#[test]
fn a_tilde_with_no_patch_reaches_the_next_major() {
    let r = range("~1.2");
    assert!(contains(&r, stable(1, 2, 0)));
    assert!(contains(&r, stable(1, 9, 9)));
    assert!(!contains(&r, stable(2, 0, 0)));
}

#[test]
fn a_tilde_on_a_major_alone_reaches_the_next_major() {
    let r = range("~1");
    assert!(contains(&r, stable(1, 0, 0)));
    assert!(contains(&r, stable(1, 9, 9)));
    assert!(!contains(&r, stable(2, 0, 0)));
}

#[test]
fn a_tilde_on_a_branch_admits_only_that_branch() {
    let main = Version::Branch("main".to_owned());
    let r = constraint_to_range(&Constraint::Tilde(main.clone()));

    assert!(contains(&r, main));
    assert!(!contains(&r, stable(1, 0, 0)));
}

// ---------------------------------------------------------------------------
// Where a hyphen range ends
// ---------------------------------------------------------------------------

#[test]
fn a_hyphen_range_with_no_patch_on_the_right_ends_at_the_next_minor() {
    // "1.0 - 2.0": right side has no patch, so upper bound is <2.1.0.
    let r = range("1.0 - 2.0");
    assert!(contains(&r, stable(1, 0, 0)));
    assert!(contains(&r, stable(1, 9, 9)));
    assert!(contains(&r, stable(2, 0, 9)));
    assert!(!contains(&r, stable(2, 1, 0)));
    assert!(!contains(&r, stable(0, 9, 9)));
}

#[test]
fn a_hyphen_range_with_a_patch_on_the_right_includes_it() {
    // "1.0.0 - 2.1.0": right side has patch, so upper bound is <=2.1.0.
    let r = range("1.0.0 - 2.1.0");
    assert!(contains(&r, stable(1, 0, 0)));
    assert!(contains(&r, stable(2, 1, 0)));
    assert!(!contains(&r, stable(2, 1, 1)));
    assert!(!contains(&r, stable(0, 9, 9)));
}

// ---------------------------------------------------------------------------
// Combining constraints
// ---------------------------------------------------------------------------

#[test]
fn a_space_separated_pair_admits_only_their_overlap() {
    let r = range(">=1.0 <2.0");
    assert!(contains(&r, stable(1, 0, 0)));
    assert!(contains(&r, stable(1, 9, 9)));
    assert!(!contains(&r, stable(0, 9, 9)));
    assert!(!contains(&r, stable(2, 0, 0)));
}

#[test]
fn a_double_pipe_admits_either_side() {
    let r = range("1.0 || 2.0");
    assert!(contains(&r, stable(1, 0, 0)));
    assert!(contains(&r, stable(2, 0, 0)));
    assert!(!contains(&r, stable(1, 5, 0)));
}

#[test]
fn two_bounded_ranges_leave_the_gap_between_them_out() {
    let r = range(">=1.0 <1.5 || >=2.0 <2.5");
    assert!(contains(&r, stable(1, 0, 0)));
    assert!(contains(&r, stable(1, 4, 9)));
    assert!(!contains(&r, stable(1, 5, 0)));
    assert!(!contains(&r, stable(1, 9, 0)));
    assert!(contains(&r, stable(2, 0, 0)));
    assert!(contains(&r, stable(2, 4, 9)));
    assert!(!contains(&r, stable(2, 5, 0)));
}

#[test]
fn bounds_that_cannot_both_hold_admit_nothing() {
    let r = range(">=2.0 <1.0");

    // An unsatisfiable requirement is an empty interval rather than an error, so
    // it is the solver that reports the conflict and says which pair caused it.
    assert_eq!(r, Ranges::empty());
    assert!(!contains(&r, stable(1, 5, 0)));
}

// ---------------------------------------------------------------------------
// A branch is a point of its own
// ---------------------------------------------------------------------------

#[test]
fn a_branch_constraint_admits_only_that_branch() {
    let r = constraint_to_range(&Constraint::Branch("main".to_owned()));
    assert!(contains(&r, Version::Branch("main".to_owned())));
    assert!(!contains(&r, Version::Branch("master".to_owned())));
    assert!(!contains(&r, stable(1, 0, 0)));
}

// ---------------------------------------------------------------------------
// Stability ordering at an upper bound
// ---------------------------------------------------------------------------

#[test]
fn an_exclusive_upper_bound_excludes_every_pre_release_of_that_version() {
    // ^1.0 upper bound is 2.0.0.0-Dev. Dev is the minimum stability,
    // so 2.0.0.0-alpha (which is > Dev) must also be excluded.
    let r = range("^1.0");
    assert!(!contains(&r, dev(2, 0, 0)));
    assert!(!contains(&r, version(2, 0, 0, Stability::Alpha, 0)));
    assert!(!contains(&r, version(2, 0, 0, Stability::Beta, 0)));
    assert!(!contains(&r, version(2, 0, 0, Stability::RC, 0)));
    assert!(!contains(&r, stable(2, 0, 0)));
}

// ---------------------------------------------------------------------------
// Reading a version back out of a range
// ---------------------------------------------------------------------------

#[test]
fn an_included_lower_bound_is_returned_as_it_stands() {
    // The answer stands in for a virtual package with no providers yet, so any
    // version inside the range serves; the lower bound is the cheapest to reach.
    assert_eq!(range_to_version(&range("^1.2.3")), stable(1, 2, 3));
    assert_eq!(range_to_version(&range(">=1.0")).to_string(), "1.0");
    assert_eq!(range_to_version(&range("1.2.3")), stable(1, 2, 3));
}

#[test]
fn an_excluded_numeric_lower_bound_is_stepped_past() {
    let r = range(">1.0");
    let version = range_to_version(&r);

    // The build segment, the fourth, is raised so the result lands strictly
    // inside a range that does not admit its own bound.
    assert_eq!(version.to_string(), "1.0.0.1");
    assert!(
        r.contains(&version),
        "the stepped version must satisfy the range",
    );
}

#[test]
fn a_range_with_no_lower_bound_yields_the_zero_version() {
    // `<2.0` is bounded above only, and the union `!=1.5` opens downwards too.
    for constraint in ["<2.0", "<=2.0", "*", "!=1.5"] {
        let r = range(constraint);
        let version = range_to_version(&r);
        assert_eq!(version.to_string(), "0.0", "for {constraint}");
        assert!(r.contains(&version), "for {constraint}");
    }
}

#[test]
fn an_empty_range_yields_the_zero_version_too() {
    // Nothing can satisfy an empty interval, so there is no better answer than
    // the same floor an unbounded one gets.
    let version = range_to_version(&range(">=2.0 <1.0"));
    assert_eq!(version.to_string(), "0.0");
}

#[test]
fn a_branch_range_yields_the_branch() {
    let r = constraint_to_range(&Constraint::Branch("main".to_owned()));
    let version = range_to_version(&r);

    assert_eq!(version, Version::Branch("main".to_owned()));
    assert!(r.contains(&version));
}

// ---------------------------------------------------------------------------
// Cross-check: Constraint::matches agrees with constraint_to_range
// ---------------------------------------------------------------------------

/// `Constraint::matches` in phast-version mirrors the interval semantics of
/// `constraint_to_range` without depending on pubgrub. This table guards the
/// two implementations against drifting apart.
#[test]
fn matches_agrees_with_range_containment() {
    let constraints = [
        "*",
        "1.2.3",
        "^1.2.3",
        "^0.3.1",
        "^0.0.4",
        "^0.0",
        "~1.2.3",
        "~1.2",
        "~1",
        "1.*",
        "1.2.*",
        ">=1.0",
        ">1.0",
        "<2.0",
        "<=2.0",
        "!=1.5.0",
        ">=1.0 <2.0",
        "^1.0 || ^2.0",
        "dev-main",
        ">dev-main",
        ">=dev-main",
        "<dev-main",
        "!=dev-main",
        "1.0 - 2.0",
        "^1.0.0-beta",
    ];

    let versions = [
        stable(0, 0, 4),
        stable(0, 0, 5),
        stable(0, 1, 0),
        stable(0, 3, 9),
        stable(0, 4, 0),
        stable(1, 0, 0),
        stable(1, 2, 3),
        stable(1, 2, 9),
        stable(1, 3, 0),
        stable(1, 5, 0),
        stable(1, 9, 9),
        stable(2, 0, 0),
        stable(2, 1, 0),
        stable(3, 0, 0),
        dev(1, 2, 0),
        dev(2, 0, 0),
        version(1, 0, 0, Stability::Beta, 1),
        version(2, 0, 0, Stability::Alpha, 0),
        Version::Branch("main".to_owned()),
        Version::Branch("master".to_owned()),
    ];

    for raw in constraints {
        let constraint = parse_constraint(raw).unwrap();
        let range = constraint_to_range(&constraint);
        for version in &versions {
            assert_eq!(
                constraint.matches(version),
                range.contains(version),
                "divergence for constraint '{raw}' and version '{version}'",
            );
        }
    }
}
