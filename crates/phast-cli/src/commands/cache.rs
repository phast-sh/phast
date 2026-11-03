//! Measuring and emptying the two directories under the cache root.
//!
//! The [`Cache`] built here is deliberately host-less, unlike the one a registry provider
//! builds for itself. Without a host, `metadata_dir()` is `{root}/metadata` rather than
//! `{root}/metadata/{host}`, which is the parent of every registry's slice. Since sizing
//! recurses and clearing removes the tree, both subcommands cover every registry, and would
//! keep doing so if metadata were ever partitioned by host again.
//!
//! Sizes are best-effort and never fail the command. A directory that cannot be read counts
//! as zero, so an unreadable cache reports `0 B` and exits successfully, and `format_bytes`
//! divides down, so 1.9 MB prints as `1 MB`.
//!
//! `clear` names the cache root in its message whatever it removed, so
//! `cache clear --metadata-only` reports `Cleared cache at {root}` with `archives/` still on
//! disk. The figure beside it is measured before deletion.

use crate::output::Printer;

use std::path::{Path, PathBuf};

use phast_registry::http::cache::Cache;

use anyhow::Result;
use clap::{Args, Subcommand};

const KB: u64 = 1024;
const MB: u64 = 1024 * KB;

#[derive(Args)]
pub struct CacheArgs {
    #[command(subcommand)]
    pub command: CacheCommands,
}

/// Subcommands under `phast cache`.
#[derive(Subcommand)]
#[command(disable_help_subcommand = true)]
pub enum CacheCommands {
    /// Show how much disk space the cache is using.
    Size(SizeArgs),
    /// Delete cached metadata and/or archives.
    Clear(ClearArgs),
}

#[derive(Args)]
pub struct SizeArgs {
    /// Show only the metadata cache size.
    #[arg(short = 'm', long, conflicts_with = "archives_only")]
    pub metadata_only: bool,

    /// Show only the archive cache size.
    #[arg(short = 'a', long, conflicts_with = "metadata_only")]
    pub archives_only: bool,
}

#[derive(Args)]
pub struct ClearArgs {
    /// Clear only the metadata cache.
    #[arg(short = 'm', long, conflicts_with = "archives_only")]
    pub metadata_only: bool,

    /// Clear only the archive cache.
    #[arg(short = 'a', long, conflicts_with = "metadata_only")]
    pub archives_only: bool,
}

#[rustfmt::skip]
pub async fn run(args: CacheArgs, cache_dir: PathBuf, printer: &Printer) -> Result<()> {
    let cache = Cache::new(cache_dir);
    match args.command {
        CacheCommands::Size(args)  => size(args, &cache, printer),
        CacheCommands::Clear(args) => clear(args, &cache, printer),
    }
}

/// Print a size breakdown of the requested cache layers.
fn size(args: SizeArgs, cache: &Cache, printer: &Printer) -> Result<()> {
    if !cache.exists() {
        printer.info("Cache directory does not exist.");
        return Ok(());
    }

    let show_metadata = !args.archives_only;
    let show_archives = !args.metadata_only;

    let mut metadata_bytes = 0;
    if show_metadata {
        metadata_bytes = dir_size(&cache.metadata_dir())
    }

    let mut archives_bytes = 0;
    if show_archives {
        archives_bytes = dir_size(&cache.archives_dir())
    }

    if show_metadata {
        printer.info(format!(
            "  metadata   {}   {}",
            format_bytes(metadata_bytes),
            cache.metadata_dir().display(),
        ));
    }

    if show_archives {
        printer.info(format!(
            "  archives   {}   {}",
            format_bytes(archives_bytes),
            cache.archives_dir().display(),
        ));
    }

    if show_metadata && show_archives {
        printer.info(format!(
            "  {}\n  total      {}",
            "─".repeat(17),
            format_bytes(metadata_bytes + archives_bytes),
        ));
    }

    Ok(())
}

/// Delete the requested cache layers and report how much space was freed.
fn clear(args: ClearArgs, cache: &Cache, printer: &Printer) -> Result<()> {
    if !cache.exists() {
        printer.info("Cache directory does not exist, nothing to clear.");
        return Ok(());
    }

    let clear_metadata = !args.archives_only;
    let clear_archives = !args.metadata_only;

    let mut freed = 0;

    if clear_metadata {
        freed += clear_dir(&cache.metadata_dir())?;
    }

    if clear_archives {
        freed += clear_dir(&cache.archives_dir())?;
    }

    printer.success(format!(
        "Cleared cache at {} ({} freed).",
        cache,
        format_bytes(freed),
    ));

    Ok(())
}

/// Remove `dir` and return how many bytes were freed. Ignores `NotFound`.
fn clear_dir(dir: &Path) -> Result<u64> {
    let freed = dir_size(dir);
    if let Err(e) = std::fs::remove_dir_all(dir)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        return Err(e.into());
    }
    Ok(freed)
}

/// Recursively sum the size of all files under `dir`.
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                dir_size(&e.path())
            } else {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

/// Format a byte count as a human-readable string.
fn format_bytes(bytes: u64) -> String {
    if bytes >= MB {
        format!("{} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{} B", bytes)
    }
}
