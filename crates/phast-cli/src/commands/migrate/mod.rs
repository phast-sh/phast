//! Converting a `composer.json` into a `phast.toml`.
//!
//! Non-destructive: `composer.json` is neither removed nor modified. `composer.lock` is
//! ignored deliberately, so `phast install` afterward produces a clean `phast.lock`
//! rather than inheriting Composer's resolution.
//!
//! Writes a second file when the manifest carries credentials: `auth.toml`, which holds
//! them in plain text and is what the run warns about gitignoring.
//!
//! Thin coordinator: the conversion itself lives in `convert`.

mod convert;

use crate::args::ProjectArg;
use crate::hints;
use crate::output::Printer;
use crate::project;

use std::str::FromStr;

use phast_core::manifest::composer::ComposerManifest;
use phast_core::toml::normalize_document;

use anyhow::{Context, Result, bail};
use clap::Args;

#[derive(Args)]
pub struct MigrateArgs {
    /// Overwrite phast.toml if it already exists.
    #[arg(long)]
    pub force: bool,
    #[command(flatten)]
    pub project: ProjectArg,
}

#[rustfmt::skip]
pub async fn run(args: MigrateArgs, printer: &Printer) -> Result<()> {
    let workdir = project::workdir(args.project.dir())?;

    let composer_json = workdir.join("composer.json");
    if !composer_json.exists() {
        bail!(hints::composer_json_not_found());
    }

    let phast_toml = workdir.join("phast.toml");
    if phast_toml.exists() && !args.force {
        bail!(hints::phast_toml_exists());
    }

    let content = std::fs::read_to_string(&composer_json)
        .context("failed to read composer.json")?;
    let composer = ComposerManifest::from_str(&content)
        .context("failed to parse composer.json")?;

    let output = convert::build_phast_manifest(&workdir, &composer).to_toml()
        .context("failed to serialize phast.toml")?;
    std::fs::write(&phast_toml, output)
        .context("failed to write phast.toml")?;

    let fields = convert::detect_unsupported_fields(&content);
    if !fields.is_empty() {
        printer.warning(format!(
            "These composer.json fields are not yet supported and were not carried over: {}.",
            fields.join(", "),
        ));
    }

    let auth = convert::build_auth_manifest(&workdir, &composer)?;
    if !auth.is_empty() {
        let content = toml::to_string_pretty(&auth)
            .context("failed to serialize auth.toml")?;
        let output = normalize_document(&content)
            .context("failed to format auth.toml")?;
        std::fs::write(workdir.join("auth.toml"), output)
            .context("failed to write auth.toml")?;

        printer.warning(
            "Credentials were written to auth.toml. \
             Add auth.toml to your .gitignore to keep credentials out of version control.",
        );
    }

    printer.success(
        "composer.json migrated to phast.toml. Run 'phast install' to install dependencies.",
    );

    Ok(())
}
