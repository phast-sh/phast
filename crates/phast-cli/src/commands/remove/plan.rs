//! Deciding which entries `remove` drops.

use crate::hints;
use crate::package::PackageRequest;
use crate::project::manifest::{Change, Editor, Section};

use anyhow::{Result, bail};

/// The changes to apply, refusing any package the section does not declare.
///
/// A refusal discards the whole set. Changes for earlier requests are built first and then
/// dropped when collecting into a `Result`, so naming one absent package leaves the others
/// in place rather than half-removed. That holds only while building a change stays free
/// of side effects.
pub fn changes(
    requests: &[PackageRequest],
    manifest: &Editor,
    section: Section,
) -> Result<Vec<Change>> {
    requests
        .iter()
        .map(|request| {
            if !manifest.contains(section, &request.name) {
                bail!(hints::package_not_found(&request.name, &section));
            }

            Ok(Change::Remove {
                package: request.name.clone(),
            })
        })
        .collect()
}
