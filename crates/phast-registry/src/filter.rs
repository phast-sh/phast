//! The rules a registry version has to pass to be offered to the resolver.
//!
//! A registry lists more versions than a project may use. [`VersionFilter`] is one rule,
//! built from the manifest, and [`Rejection`] is the reason it gives for a version it does
//! not pass, kept so a failed resolve can say why a version was not on offer.

use std::collections::HashMap;
use std::time::Duration;

use phast_core::package::PackageName;
use phast_version::{Stability, Version};

use chrono::Duration as ChronoDuration;
use chrono::{DateTime, FixedOffset, Utc};

use crate::p2::convert::determine_version_stability;
use crate::router::RegistryOptions;

/// One rule a registry version has to pass to be offered.
#[derive(Clone, Debug)]
pub enum VersionFilter {
    /// The stability floor, lowered for the packages a root `@stability` flag names.
    Stability {
        floor: Stability,
        overrides: HashMap<PackageName, Stability>,
    },
    /// Versions published after `cutoff` are not offered; `window` is the configured age.
    Age {
        cutoff: DateTime<Utc>,
        window: Duration,
    },
}

/// Why a filter did not offer a version.
#[derive(Clone, Debug, PartialEq)]
pub enum Rejection {
    /// Less stable than `floor`, the floor that applied to the package.
    Stability { floor: Stability },
    /// Published at `published`, inside the `window` the project waits.
    Age {
        published: DateTime<FixedOffset>,
        window: Duration,
    },
}

impl VersionFilter {
    /// The filters `options` configure, in the order they are applied: stability always, age
    /// when `minimum_package_age` is set.
    ///
    /// The age cutoff is taken from the clock here, once, so a whole resolve judges every
    /// version against the same instant.
    pub fn all(options: &RegistryOptions) -> Vec<Self> {
        let stability = Self::Stability {
            floor: options.minimum_stability,
            overrides: options.stability_overrides.clone(),
        };

        let age = options.minimum_package_age.map(|window| Self::Age {
            cutoff: Utc::now() - ChronoDuration::from_std(window).unwrap_or(ChronoDuration::zero()),
            window,
        });

        std::iter::once(stability).chain(age).collect()
    }

    /// Pass `version` of `name`, published at `published` when the registry says, or reject it.
    ///
    /// A version with no publish time passes the age filter: a registry that publishes none
    /// would otherwise have every release held back for good, and a freshly uploaded version
    /// always carries one.
    pub fn check(
        &self,
        name: &PackageName,
        version: &Version,
        published: Option<DateTime<FixedOffset>>,
    ) -> Result<(), Rejection> {
        match self {
            Self::Stability { floor, overrides } => {
                let floor = overrides.get(name).copied().unwrap_or(*floor);
                if determine_version_stability(version) < floor {
                    return Err(Rejection::Stability { floor });
                }
            }
            Self::Age { cutoff, window } => {
                if let Some(published) = published.filter(|published| published >= cutoff) {
                    return Err(Rejection::Age {
                        published,
                        window: *window,
                    });
                }
            }
        }

        Ok(())
    }
}
