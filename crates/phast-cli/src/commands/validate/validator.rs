//! The runner: [`Validator`] holds the rules to run and applies each to one scope,
//! attaching the rule's severity to every finding it reports.

use super::rule::{Finding, ValidationContext, ValidationRule};

use phast_workspace::Scope;

/// Checks one part of a project offline against the rules it is given.
///
/// Parse and type validation happened when the scope was resolved, through the same
/// pipeline `install` uses. The only I/O left here is checking that a `path` source exists.
pub struct Validator<'a> {
    scope: &'a Scope<'a>,
    rules: &'a [Box<dyn ValidationRule>],
}

impl<'a> Validator<'a> {
    /// Check the part of a project `scope` addresses.
    ///
    /// The scope decides all three inputs: the manifest to check, the directory `path`
    /// sources resolve against, and the member every finding is attributed to.
    pub fn new(scope: &'a Scope<'a>) -> Self {
        Self { scope, rules: &[] }
    }

    /// Set the rules to run. The caller composes the list.
    ///
    /// Borrowed rather than owned, so one list serves every scope a run covers: a boxed rule
    /// is not `Clone`, and taking ownership would mean rebuilding the list per member.
    pub fn with_rules(mut self, rules: &'a [Box<dyn ValidationRule>]) -> Self {
        self.rules = rules;
        self
    }

    /// Run every rule and collect their findings.
    ///
    /// Findings are sorted (errors first, then by message) so the output is stable across
    /// runs; the rules themselves iterate `HashMap`s in an unspecified order.
    #[rustfmt::skip]
    pub fn validate(&self) -> Vec<Finding> {
        let ctx = ValidationContext {
            manifest: self.scope.manifest(),
            workdir:  self.scope.dir(),
        };

        let mut findings = Vec::new();
        for rule in self.rules {
            let severity = rule.severity();
            let rule_id  = rule.id();

            for violation in rule.apply(&ctx) {
                findings.push(Finding {
                    rule: rule_id,
                    severity,
                    violation,
                });
            }
        }

        findings.sort_by(|a, b| {
            a.severity
                .rank()
                .cmp(&b.severity.rank())
                .then_with(|| a.violation.message.cmp(&b.violation.message))
        });
        findings
    }
}
