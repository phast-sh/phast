//! The concrete validation rules. Each is a small type implementing [`ValidationRule`];
//! the caller composes which ones to run.

use super::rule::{Rule, Severity, ValidationContext, ValidationRule, Violation};

use std::collections::HashSet;

use phast_core::manifest::{Repository, Source};
use phast_core::package::PackageName;

/// A package must not sit in both `[require]` and `[require-dev]`.
pub struct DuplicateDependency;

impl ValidationRule for DuplicateDependency {
    fn id(&self) -> Rule {
        Rule::DuplicateDependency
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn apply(&self, ctx: &ValidationContext) -> Vec<Violation> {
        ctx.manifest
            .require
            .keys()
            .filter(|name| ctx.manifest.require_dev.contains_key(*name))
            .map(|name| {
                Violation::new(format!(
                    "'{}' is in both [require] and [require-dev]",
                    name.as_str(),
                ))
                .hint("keep it in one section")
            })
            .collect()
    }
}

/// A package must not depend on itself.
pub struct SelfDependency;

impl ValidationRule for SelfDependency {
    fn id(&self) -> Rule {
        Rule::SelfDependency
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn apply(&self, ctx: &ValidationContext) -> Vec<Violation> {
        let Some(name) = &ctx.manifest.name else {
            return Vec::new();
        };

        let package = PackageName::new(name);
        if ctx.manifest.require.contains_key(&package)
            || ctx.manifest.require_dev.contains_key(&package)
        {
            vec![
                Violation::new(format!("'{name}' requires itself",))
                    .hint("remove it from [require] and [require-dev]"),
            ]
        } else {
            Vec::new()
        }
    }
}

/// A `[sources]` `{ repository }` must name a defined `[[repositories]]` entry.
pub struct DanglingRepository;

impl ValidationRule for DanglingRepository {
    fn id(&self) -> Rule {
        Rule::DanglingRepository
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn apply(&self, ctx: &ValidationContext) -> Vec<Violation> {
        let repositories: HashSet<&str> = ctx
            .manifest
            .repositories
            .iter()
            .filter_map(repository_name)
            .collect();

        ctx.manifest
            .sources
            .iter()
            .filter_map(|(package, source)| match source {
                Source::Repository { repository } if !repositories.contains(repository.as_str()) => {
                    Some(
                        Violation::new(format!(
                            "[sources] '{}' names repository '{repository}', which is not defined in [[repositories]]",
                            package.as_str(),
                        ))
                        .hint("define it in [[repositories]], or fix the name"),
                    )
                }
                _ => None,
            })
            .collect()
    }
}

/// A `[sources]` entry should name a package the project actually depends on.
pub struct OrphanSource;

impl ValidationRule for OrphanSource {
    fn id(&self) -> Rule {
        Rule::OrphanSource
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn apply(&self, ctx: &ValidationContext) -> Vec<Violation> {
        ctx.manifest
            .sources
            .keys()
            .filter(|package| {
                !ctx.manifest.require.contains_key(*package)
                    && !ctx.manifest.require_dev.contains_key(*package)
            })
            .map(|package| {
                Violation::new(format!(
                    "[sources] '{}' is not in [require] or [require-dev]",
                    package.as_str(),
                ))
                .hint("remove the source, or add the package to your dependencies")
            })
            .collect()
    }
}

/// A `[sources]` path source must point at a directory that exists.
pub struct MissingPathSource;

impl ValidationRule for MissingPathSource {
    fn id(&self) -> Rule {
        Rule::MissingPathSource
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn apply(&self, ctx: &ValidationContext) -> Vec<Violation> {
        ctx.manifest
            .sources
            .iter()
            .filter_map(|(package, source)| match source {
                Source::Path { path, .. } if !ctx.workdir.join(path).is_dir() => Some(
                    Violation::new(format!(
                        "[sources] '{}' points at '{path}', which does not exist",
                        package.as_str(),
                    ))
                    .hint("create the directory, or fix the path"),
                ),
                _ => None,
            })
            .collect()
    }
}

/// The name a repository is referenced by from `[sources]`, if it is a named registry.
#[rustfmt::skip]
fn repository_name(repository: &Repository) -> Option<&str> {
    match repository {
          Repository::Composer  { name, .. }
        | Repository::Packagist { name, .. }
        | Repository::Vcs       { name, .. } => name.as_deref(),
                                           _ => None,
    }
}
