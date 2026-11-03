use std::path::Path;

use phast_cli::commands::validate::{
    DanglingRepository, DuplicateDependency, Finding, MissingPathSource, OrphanSource,
    SelfDependency, Severity, ValidationRule, Validator,
};

use phast_workspace::Workspace;

use indoc::indoc;

/// The standard rule set (the same one `phast validate` runs).
fn rules() -> Vec<Box<dyn ValidationRule>> {
    vec![
        Box::new(DuplicateDependency),
        Box::new(SelfDependency),
        Box::new(DanglingRepository),
        Box::new(OrphanSource),
        Box::new(MissingPathSource),
    ]
}

fn validate(toml: &str) -> Vec<Finding> {
    validate_with(toml, |_| {})
}

/// Write `toml` as `phast.toml` in a fresh temp dir and return the findings. `setup` runs
/// against the temp dir first (e.g. to create path-source targets).
fn validate_with(toml: &str, setup: impl FnOnce(&Path)) -> Vec<Finding> {
    let dir = tempfile::tempdir().expect("temp dir");
    setup(dir.path());
    std::fs::write(dir.path().join("phast.toml"), toml).expect("write phast.toml");

    // Standalone, so the scope narrows to nothing and the manifest is the project's own.
    let workspace = Workspace::resolve(dir.path()).expect("manifest loads");
    let scope = workspace.scope(dir.path(), None).expect("scope");

    Validator::new(&scope).with_rules(&rules()).validate()
}

fn errors(findings: &[Finding]) -> usize {
    findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::Error))
        .count()
}

fn warnings(findings: &[Finding]) -> usize {
    findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::Warning))
        .count()
}

fn joined(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| f.violation.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Each finding as `(severity, rule id)`, since neither type is `Debug`.
fn report(findings: &[Finding]) -> Vec<(&'static str, &'static str)> {
    findings
        .iter()
        .map(|f| {
            let severity = match f.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            (severity, f.rule.as_str())
        })
        .collect()
}

// ---------------------------------------------------------------------------
// What each rule reports
// ---------------------------------------------------------------------------

#[test]
fn a_clean_manifest_has_no_findings() {
    let manifest = indoc! {r#"
        [package]
        name = "acme/app"
        type = "project"

        [require]
        "monolog/monolog" = "^3.0"
    "#};
    let findings = validate(manifest);
    assert!(
        findings.is_empty(),
        "unexpected findings: {}",
        joined(&findings),
    );
}

#[test]
fn a_package_in_require_and_require_dev_is_an_error() {
    let manifest = indoc! {r#"
        [require]
        "acme/x" = "^1.0"
        [require-dev]
        "acme/x" = "^1.0"
    "#};
    let findings = validate(manifest);
    assert_eq!(errors(&findings), 1);
    assert!(joined(&findings).contains("'acme/x' is in both [require] and [require-dev]"));
}

#[test]
fn a_package_that_requires_itself_is_an_error() {
    let manifest = indoc! {r#"
        [package]
        name = "acme/app"

        [require]
        "acme/app" = "*"
    "#};
    let findings = validate(manifest);
    assert_eq!(errors(&findings), 1);
    assert!(joined(&findings).contains("'acme/app' requires itself"));
}

#[test]
fn a_sources_entry_naming_an_unknown_repository_is_an_error() {
    let manifest = indoc! {r#"
        [require]
        "acme/x" = "*"

        [sources]
        "acme/x" = { repository = "does-not-exist" }
    "#};
    let findings = validate(manifest);
    assert_eq!(errors(&findings), 1);
    assert!(joined(&findings).contains("names repository 'does-not-exist'"));
}

#[test]
fn a_sources_entry_naming_a_declared_repository_is_fine() {
    let manifest = indoc! {r#"
        [[repositories]]
        name = "internal"
        type = "composer"
        url  = "https://packages.acme.example"

        [require]
        "acme/x" = "*"

        [sources]
        "acme/x" = { repository = "internal" }
    "#};
    let findings = validate(manifest);
    assert!(
        findings.is_empty(),
        "unexpected findings: {}",
        joined(&findings),
    );
}

#[test]
fn a_sources_entry_naming_a_declared_vcs_repository_is_fine() {
    let manifest = indoc! {r#"
        [[repositories]]
        name = "acme-mono"
        type = "vcs"
        url  = "https://github.com/acme/monorepo"

        [require]
        "acme/orders" = "*"

        [sources]
        "acme/orders" = { repository = "acme-mono" }
    "#};
    let findings = validate(manifest);
    assert!(
        findings.is_empty(),
        "a named vcs repository must be referenceable from [sources]: {}",
        joined(&findings),
    );
}

#[test]
fn a_source_for_an_undeclared_package_is_a_warning() {
    let manifest = indoc! {r#"
        [[repositories]]
        name = "internal"
        type = "composer"
        url  = "https://packages.acme.example"

        [sources]
        "acme/orphan" = { repository = "internal" }
    "#};
    let findings = validate(manifest);
    assert_eq!(warnings(&findings), 1);
    assert_eq!(errors(&findings), 0);
    assert!(joined(&findings).contains("'acme/orphan' is not in [require] or [require-dev]"));
}

#[test]
fn a_git_source_is_fine() {
    let manifest = indoc! {r#"
        [require]
        "acme/x" = "*"

        [sources]
        "acme/x" = { url = "https://github.com/acme/x", branch = "main" }
    "#};
    let findings = validate(manifest);
    assert!(
        findings.is_empty(),
        "unexpected findings: {}",
        joined(&findings),
    );
}

#[test]
fn a_path_source_at_a_missing_directory_is_a_warning() {
    let manifest = indoc! {r#"
        [require]
        "acme/x" = "*"

        [sources]
        "acme/x" = { path = "packages/missing" }
    "#};
    let findings = validate(manifest);
    assert_eq!(warnings(&findings), 1);
    assert!(joined(&findings).contains("which does not exist"));
}

#[test]
fn a_path_source_at_an_existing_directory_is_fine() {
    let manifest = indoc! {r#"
        [require]
        "acme/x" = "*"

        [sources]
        "acme/x" = { path = "packages/local" }
    "#};
    let findings = validate_with(manifest, |dir| {
        std::fs::create_dir_all(dir.join("packages/local")).unwrap()
    });
    assert!(
        findings.is_empty(),
        "unexpected findings: {}",
        joined(&findings),
    );
}

// ---------------------------------------------------------------------------
// The order findings come back in
// ---------------------------------------------------------------------------

#[test]
fn errors_come_before_warnings_and_then_message_order() {
    let manifest = indoc! {r#"
        [package]
        name = "acme/app"

        [require]
        "acme/app" = "*"

        [sources]
        "acme/zeta"  = { url = "https://github.com/acme/zeta", branch = "main" }
        "acme/alpha" = { url = "https://github.com/acme/alpha", branch = "main" }
    "#};

    let findings = validate(manifest);

    assert_eq!(
        report(&findings),
        vec![
            ("error", "self-dependency"),
            ("warning", "orphan-source"),
            ("warning", "orphan-source"),
        ],
    );

    // The two warnings come from one `[sources]` table, so only the sort decides
    // which is printed first.
    assert!(findings[1].violation.message.contains("acme/alpha"));
    assert!(findings[2].violation.message.contains("acme/zeta"));
}

#[test]
fn the_same_manifest_validates_the_same_way_twice() {
    let manifest = indoc! {r#"
        [sources]
        "acme/one"   = { url = "https://github.com/acme/one", branch = "main" }
        "acme/two"   = { url = "https://github.com/acme/two", branch = "main" }
        "acme/three" = { url = "https://github.com/acme/three", branch = "main" }
    "#};

    // The rules iterate `HashMap`s, whose order is randomized per process, so a
    // second run is the only thing that shows the sort is what fixes the report.
    assert_eq!(joined(&validate(manifest)), joined(&validate(manifest)));
}

#[test]
fn one_source_entry_can_trip_two_rules() {
    let manifest = indoc! {r#"
        [sources]
        "acme/bad" = { repository = "nope" }
    "#};

    let findings = validate(manifest);

    // Undeclared and pointed at a repository that does not exist: a rule reports
    // what it looks for, so nothing stops at the first complaint about an entry.
    assert_eq!(
        report(&findings),
        vec![
            ("error", "dangling-repository"),
            ("warning", "orphan-source"),
        ],
    );
}

// ---------------------------------------------------------------------------
// Failing before any rule runs
// ---------------------------------------------------------------------------

#[test]
fn an_invalid_constraint_fails_to_load() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = indoc! {r#"
        [require]
        "acme/x" = "!!!bogus!!!"
    "#};

    std::fs::write(dir.path().join("phast.toml"), manifest).unwrap();

    assert!(Workspace::resolve(dir.path()).is_err());
}
