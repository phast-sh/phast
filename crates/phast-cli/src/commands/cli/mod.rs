//! Dispatch for the `cli` subcommand group.
//!
//! One of two commands that nest subcommands rather than take flags, alongside `cache`.
//! Both set `disable_help_subcommand`, so `phast cli help` is rejected as an unrecognized
//! subcommand and `phast help cli` is the only way in, matching what the root `after_help`
//! tells people to use.

pub mod update;
pub mod version;

use crate::output::Printer;

use update::SelfUpdater;
use version::VersionPrinter;

use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct CliArgs {
    #[command(subcommand)]
    pub command: CliCommands,
}

#[derive(Subcommand)]
#[command(disable_help_subcommand = true)]
pub enum CliCommands {
    /// Download and apply the latest Phast release.
    Update,
    /// Show current version and last build time.
    Version,
}

#[rustfmt::skip]
pub async fn run(args: CliArgs, printer: &Printer) -> Result<()> {
    match args.command {
        CliCommands::Update => {
            SelfUpdater::new(printer).update().await
        },
        CliCommands::Version => {
            VersionPrinter::new(printer).print();
            Ok(())
        }
    }
}
