//! The project's release identity: the named-release registry, the version, and
//! the strings the CLI presents (clap's `--version` and the version block).
//!
//! The release this build ships as is [`Release::CURRENT`]; bump it (and add a
//! variant) per named release.

use std::sync::LazyLock;

/// Every named phast release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release {
    /// `0.0.x` and `0.1.0`.
    AiPetri,
}

impl Release {
    /// The release this build ships as.
    pub const CURRENT: Release = Release::AiPetri;

    /// The human-facing name, e.g. `Ai-Petri`.
    pub const fn name(self) -> &'static str {
        match self {
            Release::AiPetri => "Ai-Petri",
        }
    }
}

/// The current release's human-facing name, e.g. `Ai-Petri`.
pub const RELEASE_NAME: &str = Release::CURRENT.name();

/// The crate (and binary) version, e.g. `0.1.0`.
pub const RELEASE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The version with the release name, e.g. `0.1.0 Ai-Petri`. Used for clap's `--version`,
/// which prefixes the binary name, giving `phast 0.1.0 Ai-Petri`.
pub static RELEASE: LazyLock<String> =
    LazyLock::new(|| format!("{RELEASE_VERSION} {RELEASE_NAME}"));
