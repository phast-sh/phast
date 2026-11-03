//! The manifest's domain-model types, clustered by feature.
//!
//! These are the pieces [`Manifest`](super::Manifest) composes, and they belong to neither
//! file format: the `phast` and `composer` surfaces both read into them.
//!
//! A type deserializes itself when its shape in the file already matches its shape in
//! memory, since there is nothing for a surface to convert.

mod autoload;
mod config;
mod script;
mod source;

pub use autoload::Autoload;
pub use config::{BasicAuth, Config, PlatformCheck};
pub use script::Script;
pub use source::{GitRef, Repository, RepositoryType, Source, VcsType};
