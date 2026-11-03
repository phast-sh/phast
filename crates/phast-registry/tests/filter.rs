use std::collections::HashMap;
use std::time::Duration;

use phast_core::package::PackageName;
use phast_registry::filter::{Rejection, VersionFilter};
use phast_registry::router::RegistryOptions;
use phast_version::parser::parse_version;
use phast_version::{Stability, Version};

use chrono::{DateTime, FixedOffset, Utc};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// The instant every age filter here cuts off at.
const CUTOFF: &str = "2026-10-30T00:00:00+00:00";

/// The package every check asks about.
fn name() -> PackageName {
    PackageName::new("vendor/pkg")
}

/// The version `string` parses to.
fn version(string: &str) -> Version {
    parse_version(string).unwrap()
}

/// A stable floor, lowered to `lowered` for `vendor/pkg` when given.
fn stability(lowered: Option<Stability>) -> VersionFilter {
    VersionFilter::Stability {
        floor: Stability::Stable,
        overrides: lowered
            .map(|lowered| HashMap::from([(name(), lowered)]))
            .unwrap_or_default(),
    }
}

/// An age filter with a one-day window, cut off at [`CUTOFF`].
fn age() -> VersionFilter {
    VersionFilter::Age {
        cutoff: timestamp(CUTOFF).with_timezone(&Utc),
        window: DAY,
    }
}

/// The instant `string`, an RFC 3339 timestamp.
fn timestamp(string: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(string).unwrap()
}

// ---------------------------------------------------------------------------
// The stability floor
// ---------------------------------------------------------------------------

#[test]
fn a_version_at_the_floor_passes() {
    let filter = stability(None);
    assert_eq!(filter.check(&name(), &version("1.0.0"), None), Ok(()));
}

#[test]
fn a_version_below_the_floor_is_rejected_with_the_floor() {
    let filter = stability(None);
    assert_eq!(
        filter.check(&name(), &version("1.0.0-beta1"), None),
        Err(Rejection::Stability {
            floor: Stability::Stable
        }),
    );
}

/// `^1.0@beta` lowers the floor for its own package, and for no other.
#[test]
fn an_override_lowers_the_floor_for_its_package_only() {
    let filter = stability(Some(Stability::Beta));
    assert_eq!(filter.check(&name(), &version("1.0.0-beta1"), None), Ok(()));

    let other = PackageName::new("other/pkg");
    assert_eq!(
        filter.check(&other, &version("1.0.0-beta1"), None),
        Err(Rejection::Stability {
            floor: Stability::Stable
        }),
    );
}

// ---------------------------------------------------------------------------
// The age window
// ---------------------------------------------------------------------------

#[test]
fn a_version_published_before_the_cutoff_passes() {
    let published = timestamp("2026-10-01T00:00:00+00:00");
    assert_eq!(
        age().check(&name(), &version("1.0.0"), Some(published)),
        Ok(()),
    );
}

#[test]
fn a_version_published_after_the_cutoff_is_rejected_with_its_publish_time() {
    let published = timestamp("2026-10-30T12:00:00+00:00");
    assert_eq!(
        age().check(&name(), &version("1.0.0"), Some(published)),
        Err(Rejection::Age {
            published,
            window: DAY
        }),
    );
}

/// The window is how long a release has to have been out, so the cutoff itself is too soon.
#[test]
fn a_version_published_at_the_cutoff_is_rejected() {
    let checked = age().check(&name(), &version("1.0.0"), Some(timestamp(CUTOFF)));
    assert!(checked.is_err());
}

/// A registry that publishes no timestamps would otherwise have every release held back.
#[test]
fn a_version_with_no_publish_time_passes() {
    assert_eq!(age().check(&name(), &version("1.0.0"), None), Ok(()));
}

// ---------------------------------------------------------------------------
// Which filters a project's options build
// ---------------------------------------------------------------------------

#[test]
fn the_stability_filter_is_always_built_and_the_age_filter_only_when_set() {
    let without = VersionFilter::all(&RegistryOptions::default());
    assert!(matches!(without[..], [VersionFilter::Stability { .. }]));

    let with = VersionFilter::all(&RegistryOptions {
        minimum_package_age: Some(DAY),
        ..Default::default()
    });
    assert!(matches!(
        with[..],
        [VersionFilter::Stability { .. }, VersionFilter::Age { .. }],
    ));
}
