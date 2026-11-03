//! CLI command definitions and dispatch.
//!
//! Defines the top-level command structure, global flags, and routes each
//! subcommand to its handler. All commands are async (tokio).
//!
//! Only `phast` depends on this crate, and all it does is call [`run`]. The suites under
//! `testing/` do not link it at all, they run `target/debug/phast` and read what comes back.
//!
//! [`run`] is therefore the whole supported surface. Everything else is `pub` for one
//! reason: `tests/` beside this file compiles as its own crates, so a unit-level test of
//! `installer::vendor` or `project::lock` can reach nothing narrower. Treat those modules as
//! internal.

pub mod args;
pub mod commands;
pub mod hints;
pub mod installer;
pub mod lifecycle;
pub mod output;
pub mod package;
pub mod platform;
pub mod project;
pub mod release;
pub mod styles;

use crate::release::RELEASE;

use std::path::PathBuf;

use phast_registry::http::cache::Cache;

use anyhow::Result;
use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Parser, Subcommand};

macro_rules! dim {
    ($s:literal) => {
        concat!("\x1b[2m", $s, "\x1b[0m")
    };
}

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Green.on_default().effects(Effects::BOLD));

#[derive(Parser)]
#[command(
    name              = "phast",
    version           = RELEASE.as_str(),
    propagate_version = true,
    about             = dim!("PHP dependencies, at Rust speed."),
    after_help        = dim!("Use `phast help <command>` for more details."),
)]
#[command(styles=STYLES)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
    /// Do not output any message.
    #[arg(short, long, global = true)]
    pub quiet: bool,
    /// Increase verbosity (-v for phase timings, -vv for per-package detail).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
    /// Override the cache directory for metadata and archives.
    #[arg(long, global = true, env = "PHAST_CACHE_DIR")]
    pub cache_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new package.
    Init(commands::init::InitArgs),
    /// Install the project's dependencies.
    Install(commands::install::InstallArgs),
    /// Update dependencies to their latest allowed versions.
    Update(commands::update::UpdateArgs),
    /// Add dependencies to phast.toml.
    Add(commands::add::AddArgs),
    /// Remove dependencies from phast.toml.
    Remove(commands::remove::RemoveArgs),
    /// Resolve dependencies and write phast.lock.
    Lock(commands::lock::LockArgs),
    /// Inspect or clear the local package cache.
    Cache(commands::cache::CacheArgs),
    /// Show packages that have newer versions available.
    Outdated(commands::outdated::OutdatedArgs),
    /// Regenerate the autoloader.
    Autoload(commands::autoload::AutoloadArgs),
    /// Run a script from phast.toml.
    Run(commands::run::RunArgs),
    /// Show information about locked packages.
    Show(commands::show::ShowArgs),
    /// Show which packages require a given package.
    Why(commands::why::WhyArgs),
    /// Check installed packages for known security vulnerabilities.
    Audit(commands::audit::AuditArgs),
    /// Show the workspace root and its members.
    Workspace(commands::workspace::WorkspaceArgs),
    /// Manage the Phast executable.
    Cli(commands::cli::CliArgs),
    /// Format the project's manifests.
    Fmt(commands::fmt::FmtArgs),
    /// Validate the project's manifests.
    Validate(commands::validate::ValidateArgs),
    /// Migrate composer.json to phast.toml.
    Migrate(commands::migrate::MigrateArgs),
}

#[rustfmt::skip]
pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    let verbosity = if cli.quiet {
        output::Verbosity::Quiet
    } else {
        match cli.verbose {
            0 => output::Verbosity::Normal,
            1 => output::Verbosity::Verbose,
            _ => output::Verbosity::VeryVerbose,
        }
    };
    let cache_dir = cli.cache_dir.unwrap_or_else(Cache::default_root);
    let printer   = output::Printer::new(verbosity);

    match cli.command {
        Commands::Init(args)      => commands::init::run(args, &printer).await,
        Commands::Install(args)   => commands::install::run(args, cache_dir, &printer).await,
        Commands::Update(args)    => commands::update::run(args, cache_dir, &printer).await,
        Commands::Add(args)       => commands::add::run(args, cache_dir, &printer).await,
        Commands::Remove(args)    => commands::remove::run(args, cache_dir, &printer).await,
        Commands::Lock(args)      => commands::lock::run(args, cache_dir, &printer).await,
        Commands::Cache(args)     => commands::cache::run(args, cache_dir, &printer).await,
        Commands::Outdated(args)  => commands::outdated::run(args, cache_dir, &printer).await,
        Commands::Autoload(args)  => commands::autoload::run(args, &printer).await,
        Commands::Run(args)       => commands::run::run(args, &printer).await,
        Commands::Show(args)      => commands::show::run(args, &printer).await,
        Commands::Why(args)       => commands::why::run(args, &printer).await,
        Commands::Audit(args)     => commands::audit::run(args, &printer).await,
        Commands::Workspace(args) => commands::workspace::run(args, &printer).await,
        Commands::Cli(args)       => commands::cli::run(args, &printer).await,
        Commands::Fmt(args)       => commands::fmt::run(args, &printer).await,
        Commands::Validate(args)  => commands::validate::run(args, &printer).await,
        Commands::Migrate(args)   => commands::migrate::run(args, &printer).await,
    }
}
