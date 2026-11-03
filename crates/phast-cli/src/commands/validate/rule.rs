//! The types a validation rule is written against.
//!
//! Severity belongs to the rule, not to the problem: [`ValidationRule::severity`] is one
//! value covering every [`Violation`] that rule returns, and a violation carries none of its
//! own. So a check is an error everywhere or a warning everywhere, and splitting it by what
//! it found means splitting it into two rules.
//!
//! Adding a rule takes three edits: a [`Rule`] variant, a [`ValidationRule`] impl in
//! `rules`, and a line in the list `run` builds.

use std::path::Path;

use phast_core::manifest::Manifest;

/// One check a manifest must pass.
///
/// A rule says only *what* is wrong. It declares one severity for everything it returns, and
/// [`Validator`](super::Validator) stamps that onto each finding; whether that severity ends
/// the run is settled later, by the error count and `--strict`.
///
/// Implementors:
/// [`DuplicateDependency`](super::DuplicateDependency),
/// [`SelfDependency`](super::SelfDependency),
/// [`DanglingRepository`](super::DanglingRepository),
/// [`OrphanSource`](super::OrphanSource),
/// [`MissingPathSource`](super::MissingPathSource), all in `rules`.
pub trait ValidationRule {
    /// Which rule this is.
    fn id(&self) -> Rule;

    /// The severity of this rule's findings.
    fn severity(&self) -> Severity;

    /// One [`Violation`] per problem found in `ctx`.
    fn apply(&self, ctx: &ValidationContext) -> Vec<Violation>;
}

/// The inputs every rule reads: the parsed manifest and where it lives on disk.
pub struct ValidationContext<'a> {
    /// The manifest under test, already parsed and type-checked.
    pub manifest: &'a Manifest,
    /// The directory a relative `path` source resolves against: the member's own directory
    /// when a member is being checked, not the workspace root.
    pub workdir: &'a Path,
}

/// The catalog of validation rules: every check Phast can run on a `phast.toml`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rule {
    DuplicateDependency,
    SelfDependency,
    DanglingRepository,
    OrphanSource,
    MissingPathSource,
}

impl Rule {
    /// Stable identifier for config, CLI, and docs.
    #[rustfmt::skip]
    pub fn as_str(self) -> &'static str {
        match self {
            Rule::DuplicateDependency => "duplicate-dependency",
            Rule::SelfDependency      => "self-dependency",
            Rule::DanglingRepository  => "dangling-repository",
            Rule::OrphanSource        => "orphan-source",
            Rule::MissingPathSource   => "missing-path-source",
        }
    }
}

/// How serious a finding is: an `Error` fails validation, a `Warning` only under `--strict`.
#[derive(Clone, Copy)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    /// Sort key so errors are reported before warnings.
    #[rustfmt::skip]
    pub(super) fn rank(self) -> u8 {
        match self {
            Severity::Error   => 0,
            Severity::Warning => 1,
        }
    }
}

/// One problem a rule found: what is wrong, and optionally how to fix it.
pub struct Violation {
    pub message: String,
    pub hint: Option<String>,
}

impl Violation {
    /// A violation with just a message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            hint: None,
        }
    }

    /// Attach a fix hint, shown dimmed beneath the message.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// One issue found in a manifest: a rule's [`Violation`] tagged with the rule and severity.
pub struct Finding {
    pub rule: Rule,
    pub severity: Severity,
    pub violation: Violation,
}
