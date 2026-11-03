use phast_version::parser::parse_version;
use phast_version::{Constraint, RequireSpec, Stability, StabilityFlag, Version, VersionSpec};

/// The version `raw` names, which every spec here is resolved against or holds.
fn version(raw: &str) -> Version {
    parse_version(raw).expect("valid version")
}

// ---------------------------------------------------------------------------
// VersionSpec: reading a replace or provide value
// ---------------------------------------------------------------------------

#[test]
fn parses_self_version() {
    assert_eq!(
        "self.version".parse::<VersionSpec>().unwrap(),
        VersionSpec::SelfVersion,
    );
}

#[test]
fn parses_wildcard() {
    assert_eq!("*".parse::<VersionSpec>().unwrap(), VersionSpec::Any);
}

#[test]
fn parses_exact_version() {
    assert_eq!(
        "1.2.3".parse::<VersionSpec>().unwrap(),
        VersionSpec::Exact(version("1.2.3")),
    );
}

#[test]
fn a_branch_is_an_exact_spec_like_any_other_version() {
    // A package may replace another at a branch, and a branch is a version, so it
    // needs no variant of its own.
    assert_eq!(
        "dev-main".parse::<VersionSpec>().unwrap(),
        VersionSpec::Exact(version("dev-main")),
    );
}

#[test]
fn trims_surrounding_whitespace() {
    assert_eq!(
        "  self.version  ".parse::<VersionSpec>().unwrap(),
        VersionSpec::SelfVersion,
    );
}

#[test]
fn rejects_a_range() {
    // A version range is not a spec; the caller skips it.
    assert!("^1.0".parse::<VersionSpec>().is_err());
    assert!(">=1.0,<2.0".parse::<VersionSpec>().is_err());
}

#[test]
fn rejects_a_value_that_names_no_version() {
    for raw in ["", "   ", "not a version"] {
        assert!(raw.parse::<VersionSpec>().is_err(), "for {raw:?}");
    }
}

// ---------------------------------------------------------------------------
// VersionSpec: resolving against the declaring package
// ---------------------------------------------------------------------------

#[test]
fn self_version_and_wildcard_resolve_to_declaring() {
    let declaring = version("12.62");
    assert_eq!(VersionSpec::SelfVersion.resolve(&declaring), declaring);
    assert_eq!(VersionSpec::Any.resolve(&declaring), declaring);
}

#[test]
fn exact_resolves_to_itself_ignoring_declaring() {
    let declaring = version("12.62");
    let exact = version("1.0.0");
    assert_eq!(VersionSpec::Exact(exact.clone()).resolve(&declaring), exact);
}

// ---------------------------------------------------------------------------
// RequireSpec: reading a require or conflict value
// ---------------------------------------------------------------------------

#[test]
fn a_range_is_a_require_spec_even_though_it_is_no_version_spec() {
    // This is the whole reason the two types exist: require and conflict hold
    // ranges, while replace and provide name one version each.
    for raw in ["^1.0", ">=1.0,<2.0", ">=1.0 <2.0"] {
        assert!(raw.parse::<RequireSpec>().is_ok(), "for {raw:?}");
        assert!(raw.parse::<VersionSpec>().is_err(), "for {raw:?}");
    }
}

#[test]
fn a_star_is_an_ordinary_constraint_rather_than_a_variant_of_its_own() {
    // `*` is a valid constraint, so unlike `VersionSpec` there is nothing to
    // special-case: it resolves to `Any` without consulting the declaring version.
    let spec = "*".parse::<RequireSpec>().unwrap();
    assert_eq!(spec.resolve(&version("12.62")), Constraint::Any);
}

#[test]
fn a_require_spec_trims_surrounding_whitespace_too() {
    assert_eq!(
        "  self.version  ".parse::<RequireSpec>().unwrap(),
        RequireSpec::SelfVersion,
    );
}

#[test]
fn rejects_a_requirement_that_is_no_constraint() {
    for raw in ["", "   ", "not a version"] {
        assert!(raw.parse::<RequireSpec>().is_err(), "for {raw:?}");
    }
}

// ---------------------------------------------------------------------------
// RequireSpec: resolving against the declaring package
// ---------------------------------------------------------------------------

#[test]
fn self_version_resolves_to_exactly_the_declaring_version() {
    // A monorepo sibling pinned at `self.version` must match that one release and
    // no other, so the constraint is exact rather than a range around it.
    let declaring = version("12.62");
    assert_eq!(
        RequireSpec::SelfVersion.resolve(&declaring),
        Constraint::Exact(declaring),
    );
}

#[test]
fn a_constraint_resolves_to_itself_ignoring_declaring() {
    let spec = "^2.0".parse::<RequireSpec>().unwrap();
    assert_eq!(spec.resolve(&version("12.62")).to_string(), "^2.0");
}

// ---------------------------------------------------------------------------
// RequireSpec: the per-package stability flag
// ---------------------------------------------------------------------------

#[test]
fn require_without_flag_asks_for_no_stability() {
    let spec = "^2.0".parse::<RequireSpec>().unwrap();
    assert_eq!(spec.stability(), None);
}

#[test]
fn require_captures_the_stability_flag() {
    assert_eq!(
        "^2.0@dev".parse::<RequireSpec>().unwrap().stability(),
        Some(StabilityFlag::Explicit(Stability::Dev)),
    );
    assert_eq!(
        ">=1.0@RC".parse::<RequireSpec>().unwrap().stability(),
        Some(StabilityFlag::Explicit(Stability::RC)),
    );
}

#[test]
fn a_flag_is_read_whatever_case_it_was_written_in() {
    // Manifests are written by hand, so `@DEV` and `@dev` have to mean the same.
    for raw in ["^2.0@dev", "^2.0@DEV", "^2.0@Dev"] {
        let spec = raw.parse::<RequireSpec>().unwrap();
        assert_eq!(
            spec.stability(),
            Some(StabilityFlag::Explicit(Stability::Dev)),
            "for {raw:?}"
        );
    }
}

#[test]
fn every_stability_can_be_named_by_a_flag() {
    let flags = [
        ("@dev", Stability::Dev),
        ("@alpha", Stability::Alpha),
        ("@beta", Stability::Beta),
        ("@RC", Stability::RC),
        ("@stable", Stability::Stable),
    ];
    for (flag, expected) in flags {
        let spec = format!("^2.0{flag}").parse::<RequireSpec>().unwrap();
        assert_eq!(
            spec.stability(),
            Some(StabilityFlag::Explicit(expected)),
            "for {flag}"
        );
    }
}

#[test]
fn self_version_never_carries_a_flag() {
    assert_eq!(
        "self.version".parse::<RequireSpec>().unwrap().stability(),
        None,
    );
}

#[test]
fn bare_flag_is_any_at_that_stability() {
    let spec = "@dev".parse::<RequireSpec>().unwrap();
    assert_eq!(
        spec.stability(),
        Some(StabilityFlag::Explicit(Stability::Dev))
    );
    assert_eq!(spec.resolve(&version("1.0.0")).to_string(), "*");
}

#[test]
fn a_flag_naming_no_stability_is_refused() {
    // The `@` is split off only when what follows names a stability, so a typo is
    // an error rather than a silently ignored suffix.
    assert!("^2.0@nonsense".parse::<RequireSpec>().is_err());
    assert!("^2.0@".parse::<RequireSpec>().is_err());
}

// ---------------------------------------------------------------------------
// RequireSpec: the stability an unstable version implies
// ---------------------------------------------------------------------------

#[test]
fn a_branch_implies_dev() {
    for raw in ["dev-master", "2.x-dev", "dev-main || ^1.0"] {
        let spec = raw.parse::<RequireSpec>().unwrap();
        assert_eq!(
            spec.stability(),
            Some(StabilityFlag::Implied(Stability::Dev)),
            "for {raw:?}"
        );
    }
}

#[test]
fn a_pre_release_implies_its_own_stability() {
    let cases = [
        ("1.0.0-beta2", Stability::Beta),
        ("^1.0-beta", Stability::Beta),
        (">=1.0-RC1 <2.0", Stability::RC),
        ("1.0.0-alpha1 || 1.0.0-beta1", Stability::Alpha),
    ];
    for (raw, expected) in cases {
        let spec = raw.parse::<RequireSpec>().unwrap();
        assert_eq!(
            spec.stability(),
            Some(StabilityFlag::Implied(expected)),
            "for {raw:?}"
        );
    }
}

#[test]
fn a_stable_range_implies_nothing() {
    // `1.0 - 2.0` parses to an upper bound carrying a `-dev` sentinel, which must
    // not read as a dev requirement: the inference looks at the text, not the tree.
    for raw in ["^1.0", ">=1.0 <2.0", "1.0 - 2.0", "~1.2.3", "1.0.*", "*"] {
        let spec = raw.parse::<RequireSpec>().unwrap();
        assert_eq!(spec.stability(), None, "for {raw:?}");
    }
}

#[test]
fn an_explicit_flag_leaves_nothing_implied() {
    let spec = "dev-master@beta".parse::<RequireSpec>().unwrap();
    assert_eq!(
        spec.stability(),
        Some(StabilityFlag::Explicit(Stability::Beta))
    );
}

#[test]
fn an_implied_stability_is_never_rendered() {
    let spec = "dev-master".parse::<RequireSpec>().unwrap();
    assert_eq!(spec.to_string(), "dev-master");
}

// ---------------------------------------------------------------------------
// Rendering either spec back to a string
// ---------------------------------------------------------------------------

#[test]
fn a_version_spec_renders_as_it_was_written() {
    for spec in ["self.version", "*", "1.2.3", "dev-main"] {
        assert_eq!(spec.parse::<VersionSpec>().unwrap().to_string(), spec);
    }
}

#[test]
fn a_require_spec_renders_its_flag_back() {
    for spec in [
        "self.version",
        "^2.0",
        "*",
        "^2.0@dev",
        ">=1.0@beta",
        "~1.2@RC",
    ] {
        assert_eq!(spec.parse::<RequireSpec>().unwrap().to_string(), spec);
    }
}

#[test]
fn rendering_settles_on_one_spelling_of_a_flag() {
    // The flag is stored as a stability rather than as the text that named it, so
    // the casing a manifest happened to use does not travel into the lock.
    let spec = "^2.0@DEV".parse::<RequireSpec>().unwrap();
    assert_eq!(spec.to_string(), "^2.0@dev");

    let spec = "^2.0@rc".parse::<RequireSpec>().unwrap();
    assert_eq!(spec.to_string(), "^2.0@RC");
}

#[test]
fn a_rendered_spec_parses_back_to_the_same_spec() {
    // The rendered string is not always the input: an exact `1.0.0` renders `1.0`,
    // since trailing zeros carry no meaning. What has to hold is that the spec
    // survives, because this is how a spec reaches a lock and comes back.
    for raw in ["self.version", "*", "1.2.3", "1.0.0", "dev-main"] {
        let one = raw.parse::<VersionSpec>().unwrap();
        let two = one.to_string().parse::<VersionSpec>().unwrap();
        assert_eq!(one, two, "for {raw:?}");
    }

    for raw in ["self.version", "^2.0", "*", "^2.0@dev", ">=1.0@beta"] {
        let one = raw.parse::<RequireSpec>().unwrap();
        let two = one.to_string().parse::<RequireSpec>().unwrap();
        assert_eq!(one, two, "for {raw:?}");
    }
}
