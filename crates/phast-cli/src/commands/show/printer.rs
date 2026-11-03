//! Rendering the lock entries `show` selected, as text or JSON.
//!
//! Four renderers, because the list and the single-package detail each exist in both
//! formats. The text pair chooses its fields; the JSON pair names only the envelope keys and
//! serializes `PackageDef` straight out of the lock for every entry, so a field added to the
//! lock struct appears in `show --format json` without anything here being touched.

use crate::output::{OutputFormat, Printer};
use crate::styles;

use std::fmt;
use std::fmt::{Display, Formatter};

use phast_core::lock::phast::{PackageDef, PhastLock};

use anyhow::{Context, Result};
use serde::Serialize;

/// Renders locked package information in the requested output format.
pub struct ShowPrinter<'a> {
    format: OutputFormat,
    printer: &'a Printer,
}

impl<'a> ShowPrinter<'a> {
    pub fn new(format: OutputFormat, printer: &'a Printer) -> Self {
        Self { format, printer }
    }

    /// Print the packages that survived scoping, grouped into production and dev.
    pub fn print_all(&self, lock: &PhastLock) -> Result<()> {
        match self.format {
            OutputFormat::Text => {
                self.printer.info(AllOutput { lock }.to_string());
                Ok(())
            }
            OutputFormat::Json => {
                self.printer.info(
                    AllJsonOutput {
                        packages: &lock.packages,
                        packages_dev: &lock.packages_dev,
                    }
                    .to_pretty_json()?,
                );
                Ok(())
            }
        }
    }

    /// Print the detail view for a single package.
    pub fn print_one(&self, package: &PackageDef) -> Result<()> {
        match self.format {
            OutputFormat::Text => {
                self.printer.info(SingleOutput { package }.to_string());
                Ok(())
            }
            OutputFormat::Json => {
                self.printer
                    .info(SingleJsonOutput { package }.to_pretty_json()?);
                Ok(())
            }
        }
    }
}

/// Renders the full package list: production group, then dev group with header.
struct AllOutput<'a> {
    lock: &'a PhastLock,
}

impl Display for AllOutput<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.lock.packages.is_empty() && self.lock.packages_dev.is_empty() {
            return write!(f, "No packages installed.");
        }

        if !self.lock.packages.is_empty() {
            write!(
                f,
                "{}",
                GroupOutput {
                    packages: &self.lock.packages,
                }
            )?;
        }

        if !self.lock.packages_dev.is_empty() {
            if !self.lock.packages.is_empty() {
                writeln!(f)?;
            }
            writeln!(f, "{}", styles::PACKAGE_HINT.apply_to("dev packages"))?;
            write!(
                f,
                "{}",
                GroupOutput {
                    packages: &self.lock.packages_dev,
                }
            )?;
        }

        Ok(())
    }
}

/// Renders one group of packages as an aligned name / version / description table.
struct GroupOutput<'a> {
    packages: &'a [PackageDef],
}

#[rustfmt::skip]
impl Display for GroupOutput<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let name_width = self.packages
            .iter()
            .map(|p| p.name.len())
            .max()
            .unwrap_or(0);
        let ver_width = self.packages
            .iter()
            .map(|p| p.version.len())
            .max()
            .unwrap_or(0);

        let mut packages: Vec<&PackageDef> = self.packages
            .iter()
            .collect();
        packages.sort_by_key(|p| &p.name);

        for package in packages {
            writeln!(f, "{:name_width$}  {:ver_width$}  {}",
                styles::PACKAGE_NAME.apply_to(&package.name),
                styles::PACKAGE_VERSION.apply_to(&package.version),
                package.description.as_deref().unwrap_or(""),
            )?;
        }
        Ok(())
    }
}

/// Renders the detail view for a single package as labeled key/value fields.
struct SingleOutput<'a> {
    package: &'a PackageDef,
}

#[rustfmt::skip]
impl SingleOutput<'_> {
    fn field(&self, f: &mut Formatter<'_>, key: &str, value: &str) -> fmt::Result {
        writeln!(f, "{:9}: {}",
            styles::DEFAULT_KEY.apply_to(key),
            styles::DEFAULT_VALUE.apply_to(value),
        )
    }

    fn requires(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(f)?;
        writeln!(f, "{}", styles::PACKAGE_NAME.apply_to("requires"))?;

        let mut dependencies: Vec<(&String, &String)> = self
            .package
            .require
            .entries
            .iter()
            .collect();
        dependencies.sort_by_key(|(k, _)| k.as_str());

        let width = self
            .package
            .require
            .entries
            .keys().map(|k| k.len())
            .max()
            .unwrap_or(0);

        for (dependency, constraint) in dependencies {
            writeln!(f, "  {:<width$}  {}",
                styles::PACKAGE_NAME.apply_to(dependency),
                styles::PACKAGE_VERSION.apply_to(constraint),
            )?;
        }
        Ok(())
    }
}

#[rustfmt::skip]
impl Display for SingleOutput<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.field(f, "name", &self.package.name)?;
        self.field(f, "version", &self.package.version)?;

        if let Some(package_type) = &self.package.package_type {
            self.field(f, "type", package_type)?;
        }
        if !self.package.license.is_empty() {
            self.field(f, "license", &self.package.license.join(", "))?;
        }
        if let Some(homepage) = &self.package.homepage {
            self.field(f, "homepage", homepage)?;
        }
        if let Some(description) = &self.package.description {
            self.field(f, "desc", description)?;
        }

        let authors = self.package.authors
            .iter()
            .filter_map(|a| {
                let name = a.name.as_deref()?;
                Some(match a.email.as_deref() {
                    Some(email) => format!("{name} <{email}>"),
                    None        => name.to_string(),
                })
            })
            .collect::<Vec<_>>()
            .join(", ");

        if !authors.is_empty() {
            self.field(f, "authors", &authors)?;
        }

        if !self.package.require.is_empty() {
            self.requires(f)?;
        }

        Ok(())
    }
}

/// Renders the full package list as JSON: `{ "packages": [...], "packages-dev": [...] }`.
#[derive(Serialize)]
struct AllJsonOutput<'a> {
    packages: &'a [PackageDef],
    #[serde(rename = "packages-dev")]
    packages_dev: &'a [PackageDef],
}

#[rustfmt::skip]
impl AllJsonOutput<'_> {
    fn to_pretty_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .context("failed to serialize show results as JSON")
    }
}

/// Renders a single package as JSON: `{ "package": { ... } }`.
#[derive(Serialize)]
struct SingleJsonOutput<'a> {
    package: &'a PackageDef,
}

#[rustfmt::skip]
impl SingleJsonOutput<'_> {
    fn to_pretty_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .context("failed to serialize package as JSON")
    }
}
