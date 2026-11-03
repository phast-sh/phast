//! One module per subcommand, each entered through the same `run`.
//!
//! Every module here declares `pub async fn run(args: <Name>Args, .., printer: &Printer)
//! -> Result<()>`, so `lib.rs` dispatches all eighteen alike. The single variation is a
//! `cache_dir: PathBuf` ahead of the printer, taken by the seven that read or write the
//! metadata and archive cache: `add`, `cache`, `install`, `lock`, `outdated`, `remove` and
//! `update`. Reaching the network is not the test, `audit` queries advisories and takes none.
//!
//! No `run` and no `*Args` struct in this tree carries a doc comment, which is a decision
//! rather than an omission. A command's unit of documentation is its module docblock, and
//! its argument fields are its help text, so a docstring on either would restate one of the
//! two. It is also why `missing_docs` is left off this crate: it would ask for thirty-eight
//! of them, one per `run` and one per `*Args`.
//!
//! What a user reads belongs on the `Commands` variant in `lib.rs`, since that is what
//! `phast --help` prints.

pub mod add;
pub mod audit;
pub mod autoload;
pub mod cache;
pub mod cli;
pub mod fmt;
pub mod init;
pub mod install;
pub mod lock;
pub mod migrate;
pub mod outdated;
pub mod remove;
pub mod run;
pub mod show;
pub mod update;
pub mod validate;
pub mod why;
pub mod workspace;
