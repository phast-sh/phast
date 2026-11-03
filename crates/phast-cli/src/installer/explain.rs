//! Explaining a failed resolve with what the registries' filters kept out.
//!
//! The solver reports the requirement it could not meet, and nothing of the versions the
//! registries never offered it. When a filter is the reason, a message that names only the
//! requirement sends the user looking for a typo, so the explanation is added here, after
//! the fact, from what the provider recorded while answering.

use crate::hints;

use phast_registry::filter::Rejection;
use phast_registry::provider::ChainedProvider;

use chrono::Duration as ChronoDuration;

/// Hint lines for the versions a filter kept out, one per version, for each package
/// `message` names.
///
/// Only named packages are listed: with `minimum-package-age` on, recent releases of many
/// packages are rejected on every resolve, and most have nothing to do with why this one
/// failed. Stability rejections are left out, since the resolver's own message already
/// points at `minimum-stability`.
pub fn rejections(provider: &ChainedProvider, message: &str) -> String {
    let mut rejections = provider.rejections();
    rejections.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));

    rejections
        .iter()
        .filter(|(name, _)| message.contains(name.as_str()))
        .flat_map(|(name, versions)| versions.iter().map(move |rejected| (name, rejected)))
        .filter_map(|(name, (version, rejection))| match rejection {
            Rejection::Age { published, window } => {
                let offered = *published + ChronoDuration::from_std(*window).unwrap_or_default();
                let hint = hints::rejected_by_age(
                    name.as_str(),
                    &version.to_string(),
                    &published.format("%Y-%m-%d").to_string(),
                    &offered.format("%Y-%m-%d").to_string(),
                );
                Some(format!("\n  {hint}"))
            }
            Rejection::Stability { .. } => None,
        })
        .collect()
}
