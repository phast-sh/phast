use std::collections::HashMap;

use phast_cli::installer::requirements::collect;
use phast_core::manifest::Manifest;
use phast_core::package::PackageName;
use phast_resolver::Requirement;
use phast_version::RequireSpec;
use phast_version::parser::parse_constraint;

/// A manifest requiring `require` in production and `require_dev` in dev, and
/// declaring nothing else.
fn manifest(require: &[(&str, &str)], require_dev: &[(&str, &str)]) -> Manifest {
    Manifest {
        require: specs(require),
        require_dev: specs(require_dev),
        ..Manifest::default()
    }
}

/// `(name, constraint)` pairs as a requirement table, with no stability flag.
fn specs(entries: &[(&str, &str)]) -> HashMap<PackageName, RequireSpec> {
    entries
        .iter()
        .map(|(name, constraint)| {
            let spec = RequireSpec::Constraint {
                constraint: parse_constraint(constraint).expect("valid constraint"),
                stability: None,
            };
            (PackageName::new(*name), spec)
        })
        .collect()
}

/// The constraint `requirement` carries, as it renders.
fn constraint(requirement: &Requirement) -> String {
    match &requirement.spec {
        RequireSpec::Constraint { constraint, .. } => constraint.to_string(),
        other => panic!("expected a constraint, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Which requirements are collected
// ---------------------------------------------------------------------------

#[test]
fn empty_manifest_produces_no_requirements() {
    assert!(collect(&manifest(&[], &[]), true).is_empty());
}

#[test]
fn production_packages_are_always_included() {
    let manifest = manifest(&[("vendor/pkg", "^1.0")], &[]);

    let reqs = collect(&manifest, false);
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].name, PackageName::new("vendor/pkg"));
}

#[test]
fn dev_packages_are_excluded_without_include_dev() {
    let manifest = manifest(&[], &[("vendor/test-pkg", "^2.0")]);

    assert!(
        collect(&manifest, false).is_empty(),
        "dev packages must not appear when include_dev=false",
    );
}

#[test]
fn dev_packages_are_included_with_include_dev() {
    let manifest = manifest(&[], &[("vendor/test-pkg", "^2.0")]);

    let reqs = collect(&manifest, true);
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].name, PackageName::new("vendor/test-pkg"));
}

#[test]
fn both_sections_are_included_with_include_dev() {
    let manifest = manifest(&[("vendor/pkg", "^1.0")], &[("vendor/test-pkg", "^2.0")]);

    let reqs = collect(&manifest, true);
    assert_eq!(reqs.len(), 2);

    let names: Vec<_> = reqs.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"vendor/pkg"));
    assert!(names.contains(&"vendor/test-pkg"));
}

// ---------------------------------------------------------------------------
// What each requirement carries
// ---------------------------------------------------------------------------

#[test]
fn only_a_dev_requirement_is_flagged_as_one() {
    let manifest = manifest(&[("vendor/pkg", "^1.0")], &[("vendor/test-pkg", "^2.0")]);

    // Dev entries are appended rather than merged, so production comes first and
    // the flag is what lets the solver tag whatever only a dev requirement reaches.
    let reqs = collect(&manifest, true);
    assert_eq!(
        reqs.iter()
            .map(|r| (r.name.as_str(), r.is_dev))
            .collect::<Vec<_>>(),
        vec![("vendor/pkg", false), ("vendor/test-pkg", true)],
    );
}

#[test]
fn the_constraint_is_carried_through_unchanged() {
    let manifest = manifest(&[("vendor/pkg", ">=1.2 <2.0")], &[]);

    let reqs = collect(&manifest, false);
    assert_eq!(constraint(&reqs[0]), ">=1.2 <2.0");
}

#[test]
fn a_package_in_both_sections_arrives_twice() {
    let manifest = manifest(&[("vendor/both", "^1.0")], &[("vendor/both", "^2.0")]);

    // Not merged here: each section contributes its own entry, and the solver
    // intersects the two. `validate` is what refuses the manifest up front.
    let reqs = collect(&manifest, true);
    assert_eq!(reqs.len(), 2);
    assert_eq!(constraint(&reqs[0]), "^1.0");
    assert_eq!(constraint(&reqs[1]), "^2.0");

    // Without dev, the second never arrives, so the clash is invisible.
    assert_eq!(collect(&manifest, false).len(), 1);
}
