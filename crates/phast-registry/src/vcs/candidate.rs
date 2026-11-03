//! Turning a repository's refs and manifest into the versions it offers.

use super::{RefKind, VcsRef};

use phast_core::manifest::Manifest;
use phast_version::parser::parse_branch_alias;
use phast_version::{Stability, Version};

/// A version the repository offers, paired with the git ref that serves it: a tag
/// or branch under its own version, or a branch under a `branch-alias` dev version.
#[derive(Clone)]
pub struct Candidate {
    /// The key the resolver matches on, which for an alias is the sentinel dev
    /// version rather than anything the repository names.
    pub version: Version,
    /// Everything needed to install and lock the candidate: the commit to fetch,
    /// and the version to record. The recorded one is `reference.version()`, so an
    /// aliased branch locks as `dev-{branch}` the way Composer writes it, not under
    /// the alias's sentinel-expanded numeric.
    pub reference: VcsRef,
}

#[rustfmt::skip]
impl Candidate {
    /// The versions a repository offers: one candidate per tag and branch, plus the
    /// dev versions branches present as via `extra.branch-alias`.
    ///
    /// A ref naming no version of its own drops out here, which is what removes a
    /// tag whose name does not parse as one.
    pub fn all(
        manifest: &Manifest,
        references: Vec<VcsRef>,
        minimum_stability: Stability,
    ) -> Vec<Candidate> {
        let aliases = Self::branch_aliases(
            manifest,
            &references,
            minimum_stability,
        );

        references
            .into_iter()
            .filter_map(|reference| {
                Some(Candidate {
                    version: reference.version()?,
                    reference,
                })
            })
            .chain(aliases)
            .collect()
    }

    /// The `extra.branch-alias` candidates: each branch presenting under a dev version
    /// a numeric require can select.
    ///
    /// Aliases are dev-stability, so they are offered only when dev is permitted
    /// (`Dev >= minimum_stability`); a stable project never has a numeric require
    /// silently satisfied by a branch. Each `"dev-{branch}": "{target}"` entry maps
    /// the parsed target to the branch ref it names. An entry is skipped as soon as
    /// any part of it fails to hold up: a key that is not `dev-{branch}`, a branch the
    /// repository does not have, a target that is not a string, or one that does not
    /// parse as a dev-version placeholder.
    fn branch_aliases(
        manifest: &Manifest,
        references: &[VcsRef],
        minimum_stability: Stability,
    ) -> Vec<Candidate> {
        if Stability::Dev < minimum_stability {
            return vec![];
        }

        let Some(map) = manifest
            .extra
            .get("branch-alias")
            .and_then(|value| value.as_object())
        else {
            return vec![];
        };

        let mut aliases = Vec::new();
        for (branch_key, target) in map {
            // A branch-alias key is `dev-{branch}`; ignore any other key shape.
            let Some(branch) = branch_key.strip_prefix("dev-") else {
                continue
            };

            // The named branch must actually exist among the repository's refs.
            let Some(reference) = references.iter().find(|reference| {
                reference.kind == RefKind::Branch &&
                reference.name == branch
            }) else {
                continue
            };

            // The alias target must be a string, e.g. `1.2.x-dev`.
            let Some(target) = target.as_str() else {
                continue
            };

            // And it must parse as a dev-version placeholder.
            let Ok(version) = parse_branch_alias(target) else {
                continue
            };

            aliases.push(Candidate {
                version: Version::Numeric(version),
                reference: reference.clone(),
            });
        }
        aliases
    }
}
