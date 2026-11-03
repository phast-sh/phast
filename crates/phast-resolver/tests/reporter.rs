use std::collections::HashMap;

use phast_core::package::PackageName;
use phast_resolver::provider::memory::MemoryProvider;
use phast_resolver::reporter::{Conflict, ConstraintSource, NoSolutionReport, VersionConflict};
use phast_resolver::{Requirement, ResolveError, ResolveOptions, resolve};
use phast_version::parser::parse_constraint;
use phast_version::{NumericVersion, RequireSpec, Stability, Version};

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

/// A production requirement on `name` at `constraint`, as the root declares one.
fn requirement(name: &str, constraint: &str) -> Requirement {
    Requirement {
        name: PackageName::new(name),
        spec: RequireSpec::Constraint {
            constraint: parse_constraint(constraint).unwrap(),
            stability: None,
        },
        is_dev: false,
    }
}

/// One entry in a package's own dependency list.
fn dependency(name: &str, constraint: &str) -> (String, phast_version::Constraint) {
    (name.to_owned(), parse_constraint(constraint).unwrap())
}

/// A registry where `vendor/a` and `vendor/b` each require a different major of
/// `vendor/shared`. The report for this one renders real ranges, which is what
/// makes it the scenario worth checking the sanitation against.
fn peers() -> MemoryProvider {
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/shared", stable(1, 0, 0), vec![]);
    provider.add_package("vendor/shared", stable(2, 0, 0), vec![]);
    provider.add_package(
        "vendor/a",
        stable(1, 0, 0),
        vec![dependency("vendor/shared", "^1.0")],
    );
    provider.add_package(
        "vendor/b",
        stable(1, 0, 0),
        vec![dependency("vendor/shared", "^2.0")],
    );
    provider
}

/// The report `reqs` produce against `provider`, or a panic if they resolve.
async fn conflict(provider: MemoryProvider, reqs: Vec<Requirement>) -> String {
    match resolve(reqs, provider, HashMap::new(), ResolveOptions::default()).await {
        Err(ResolveError::Conflict(message)) => message,
        Ok(_) => panic!("expected a conflict, the requirements resolved"),
        Err(error) => panic!("expected a conflict, got: {error}"),
    }
}

/// The report for two peers demanding different majors of `vendor/shared`.
async fn peer_conflict() -> String {
    conflict(
        peers(),
        vec![
            requirement("vendor/a", "^1.0"),
            requirement("vendor/b", "^1.0"),
        ],
    )
    .await
}

// ---------------------------------------------------------------------------
// Formatting a report on its own
// ---------------------------------------------------------------------------

#[test]
fn a_version_conflict_lists_every_party_that_constrains_the_package() {
    let report = NoSolutionReport {
        conflict: Conflict::Version(VersionConflict {
            package: "symfony/polyfill-mbstring".to_owned(),
            sources: vec![
                ConstraintSource {
                    requirer: "your project".to_owned(),
                    range: ">=99.0, <100.0".to_owned(),
                },
                ConstraintSource {
                    requirer: "symfony/http-foundation[>=7.0, <8.0]".to_owned(),
                    range: ">=1.1, <2.0".to_owned(),
                },
            ],
        }),
    };

    let output = format!("{report}");
    assert!(
        output.contains("Phast could not find compatible versions"),
        "missing header: {output}",
    );
    assert!(
        output.contains("╰─▶ symfony/polyfill-mbstring has incompatible constraints:"),
        "missing conflict line: {output}",
    );
    assert!(
        output.contains("· your project requires symfony/polyfill-mbstring[>=99.0, <100.0]"),
        "missing root source: {output}",
    );
    assert!(
        output.contains(
            "· symfony/http-foundation[>=7.0, <8.0] requires symfony/polyfill-mbstring[>=1.1, <2.0]"
        ),
        "missing dep source: {output}",
    );
}

#[test]
fn a_version_conflict_is_not_limited_to_two_parties() {
    let source = |requirer: &str, range: &str| ConstraintSource {
        requirer: requirer.to_owned(),
        range: range.to_owned(),
    };
    let report = NoSolutionReport {
        conflict: Conflict::Version(VersionConflict {
            package: "psr/log".to_owned(),
            sources: vec![
                source("your project", ">=3.0, <4.0"),
                source("monolog/monolog[>=3.0, <4.0]", ">=2.0, <3.0"),
                source("symfony/console", ">=1.0, <2.0"),
            ],
        }),
    };

    // One line per requirer, so a package contested by three dependents reads as
    // three lines rather than the first two and a truncation.
    let output = format!("{report}");
    assert_eq!(output.matches("requires psr/log[").count(), 3, "{output}");

    // An unconstrained requirer carries no brackets of its own.
    assert!(
        output.contains("· symfony/console requires psr/log[>=1.0, <2.0]"),
        "{output}",
    );
}

#[test]
fn a_package_no_source_offers_is_named_as_missing() {
    // Display only: a root requirement for an unknown package is caught before
    // the solver runs, and a transitive one is treated as a virtual package, so
    // nothing in a real solve reaches this arm today.
    let report = NoSolutionReport {
        conflict: Conflict::NotFound {
            package: "vendor/ghost".to_owned(),
        },
    };

    let output = format!("{report}");
    assert!(
        output.contains("╰─▶ vendor/ghost is not available on Packagist"),
        "{output}",
    );
}

#[test]
fn a_package_with_no_satisfying_version_names_the_range() {
    let report = NoSolutionReport {
        conflict: Conflict::NoMatch {
            package: "psr/container".to_owned(),
            range: ">=3.0, <4.0".to_owned(),
        },
    };

    let output = format!("{report}");
    assert!(
        output.contains("no version of psr/container satisfies [>=3.0, <4.0]"),
        "{output}",
    );
}

#[test]
fn a_conflict_that_matched_no_pattern_is_passed_through_as_written() {
    // The fallback for a derivation tree the walk does not recognize, and for a
    // provider's own unavailability reason. It still gets the header and the arrow.
    let report = NoSolutionReport {
        conflict: Conflict::Unknown("vendor/a is unavailable: the tag was deleted".to_owned()),
    };

    let output = format!("{report}");
    assert!(
        output.contains("Phast could not find compatible versions"),
        "{output}",
    );
    assert!(
        output.contains("╰─▶ vendor/a is unavailable: the tag was deleted"),
        "{output}",
    );
}

// ---------------------------------------------------------------------------
// Conflicts a real solve produces
// ---------------------------------------------------------------------------

#[tokio::test]
async fn two_root_constraints_that_cannot_both_hold_blame_the_project() {
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(1, 0, 0), vec![]);
    provider.add_package("vendor/a", stable(2, 0, 0), vec![]);

    let message = conflict(
        provider,
        vec![
            requirement("vendor/a", "^1.0"),
            requirement("vendor/a", "^2.0"),
        ],
    )
    .await;

    // The two ranges are intersected to nothing before the solver sees them, so
    // there is no pair of requirers to list; the project itself is the answer.
    assert!(
        message.contains("no version of vendor/a satisfies [your project's requirements]"),
        "{message}",
    );
}

#[tokio::test]
async fn a_version_no_release_satisfies_names_the_range_that_was_asked_for() {
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/a", stable(1, 0, 0), vec![]);
    provider.add_package("vendor/a", stable(2, 0, 0), vec![]);

    let message = conflict(provider, vec![requirement("vendor/a", "^9.0")]).await;

    // The package is served, just not at anything matching, so the range the
    // solver was left holding is what the reader needs to see.
    assert!(
        message.contains("no version of vendor/a satisfies [>=9.0, <10.0]"),
        "{message}",
    );
}

#[tokio::test]
async fn two_dependents_wanting_different_majors_name_all_three_packages() {
    let message = peer_conflict().await;

    // Neither dependent is at fault on its own, so both are named alongside the
    // package they disagree about, each with the range it asked for.
    assert!(
        message.contains("vendor/shared has incompatible constraints:"),
        "{message}",
    );
    assert!(
        message.contains("vendor/a[>=1.0, <2.0] requires vendor/shared[>=1.0, <2.0]"),
        "{message}",
    );
    assert!(
        message.contains("requires vendor/shared[>=2.0, <3.0]"),
        "{message}",
    );
}

#[tokio::test]
async fn a_root_and_a_dependency_disagreeing_name_the_one_in_between() {
    // The root requires vendor/shared ^2.0 directly, and vendor/a requires ^1.0.
    let mut provider = MemoryProvider::new();
    provider.add_package("vendor/shared", stable(1, 0, 0), vec![]);
    provider.add_package("vendor/shared", stable(2, 0, 0), vec![]);
    provider.add_package(
        "vendor/a",
        stable(1, 0, 0),
        vec![dependency("vendor/shared", "^1.0")],
    );

    let message = conflict(
        provider,
        vec![
            requirement("vendor/a", "^1.0"),
            requirement("vendor/shared", "^2.0"),
        ],
    )
    .await;

    // Naming vendor/a is the whole point: the reader did not write the ^1.0 and
    // cannot act on the report without being told who did.
    assert!(message.contains("vendor/shared"), "{message}");
    assert!(message.contains("vendor/a"), "{message}");
    assert!(message.contains("your project"), "{message}");
}

// ---------------------------------------------------------------------------
// Sanitizing the solver's internal tokens
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_internal_root_sentinel_never_reaches_the_reader() {
    // `<root>` is PubGrub's name for the project. Checked against the peer
    // report because that one renders requirers, so the label is on the page.
    let message = peer_conflict().await;

    assert!(!message.contains("<root>"), "{message}");
    assert!(message.contains("requires"), "unexpectedly bare: {message}");
}

#[tokio::test]
async fn the_synthetic_root_version_never_reaches_the_reader() {
    // The root is assigned 0.0.0.0 so the solver has a version to anchor on.
    let message = peer_conflict().await;

    assert!(!message.contains("0.0.0.0"), "{message}");
}

#[tokio::test]
async fn the_dev_suffix_on_an_upper_bound_is_stripped() {
    // An exclusive upper bound is `<2.0-dev` internally, since a dev version
    // sorts below the release. The peer report prints two such bounds, so the
    // absence of the suffix here is the stripping working rather than a message
    // that happened to carry no ranges.
    let message = peer_conflict().await;

    assert!(
        message.contains("<2.0"),
        "no upper bound rendered: {message}",
    );
    assert!(!message.contains("-dev"), "{message}");
}
