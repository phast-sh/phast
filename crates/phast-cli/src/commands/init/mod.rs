//! Scaffolding a new package.
//!
//! Every field the wizard asks about has a flag, and a flag skips its own prompt,
//! so partial answers work: `phast init --name acme/blog` still asks for the rest.
//! Passing `--yes`, or running without a terminal on stdin, skips the prompts
//! entirely and takes the defaults.

mod wizard;

use crate::output::Printer;
use crate::project;

use wizard::{InitOptions, Wizard};

use std::io::IsTerminal;
use std::path::PathBuf;
use std::str::FromStr;

use phast_core::{PackageName, PackageType};

use anyhow::{Context, Result};
use clap::Args;

#[derive(Args)]
pub struct InitArgs {
    /// Directory to initialize. Created if it does not exist.
    /// Defaults to the current directory.
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,
    /// Package name, e.g. `acme/blog`. Defaults to `$USER/<directory>`.
    #[arg(long = "name", value_name = "VENDOR/PACKAGE", value_parser = PackageName::from_str)]
    pub package_name: Option<PackageName>,
    /// Package type: `library`, `project`, or `phast-plugin`. Defaults to `library`.
    #[arg(long = "type", value_name = "TYPE", value_parser = PackageType::from_str)]
    pub package_type: Option<PackageType>,
    /// Package description. Omitted from the manifest when not given.
    #[arg(long, value_name = "TEXT")]
    pub description: Option<String>,
    /// PHP version constraint, e.g. `^8.4`. Defaults to the detected runtime.
    #[arg(long = "php", value_name = "CONSTRAINT")]
    pub php_version: Option<String>,
    /// Do not create a `src/` directory.
    #[arg(long)]
    pub no_src: bool,
    /// Do not create a `tests/` directory.
    #[arg(long)]
    pub no_tests: bool,
    /// Take the defaults for everything not given as a flag, without prompting.
    #[arg(short, long)]
    pub yes: bool,
}

#[rustfmt::skip]
pub async fn run(args: InitArgs, printer: &Printer) -> Result<()> {
    let InitArgs {
        path,
        package_name,
        package_type,
        description,
        php_version,
        no_src,
        no_tests,
        yes,
    } = args;

    let workdir = project::workdir(path)?;
    let options = InitOptions {
        package_name,
        package_type,
        description,
        php_version,
        with_src: no_src.then_some(false),
        with_tests: no_tests.then_some(false),
        interactive: is_interactive(yes),
    };

    let manifest = Wizard::new(options, printer)
        .scaffold(&workdir)?;
    let content = manifest.to_toml()
        .context("failed to serialize phast.toml")?;
    std::fs::write(workdir.join("phast.toml"), &content)
        .context("failed to write phast.toml")?;

    printer.success(format!("Initialized package in '{}'", workdir.display()));
    printer.hint("Run `phast install` to install dependencies.");

    Ok(())
}

/// Whether the wizard may ask questions.
///
/// `--yes` is the explicit opt-out. The terminal check covers the callers that never
/// had a chance to opt out, such as a pipe, a CI job, or a test harness: prompting
/// there blocks on a read nobody is going to answer.
fn is_interactive(yes: bool) -> bool {
    !yes && std::io::stdin().is_terminal()
}
