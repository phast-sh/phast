//! Leaf types shared between the `phast.toml` manifest model (`manifest::phast`)
//! and the `phast.lock` model (`lock::phast`).
//!
//! These are small, purely informational records that both files carry identically,
//! so they live here rather than being duplicated per format.
//!
//! The crate root is deliberate: neither side owns them. Putting them under `manifest`
//! would make every locked package import from the manifest model for records that say
//! nothing about manifests, and putting them under `lock` inverts the same problem.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// An author of a package.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Author {
    /// Full name of the author.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Contact email address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Author's homepage or profile URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,

    /// Role within the project, e.g. `"maintainer"` or `"contributor"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// Support channels for a package, each mapping a channel name to a URL or address,
/// e.g. `issues`, `source`, `docs`, `email`, `chat`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Support {
    /// Flattened, so each channel is a key of the `support` table itself
    /// (`issues = "https://github.com/acme/demo/issues"`) instead of nesting
    /// under a field name.
    #[serde(flatten)]
    pub entries: BTreeMap<String, String>,
}

impl Support {
    /// Returns `true` when no support channel is set, so the section is omitted.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
