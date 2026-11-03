//! PHP file writer.
//!
//! Renders `AutoloadMaps` to the PHP files under `vendor/phast/` and writes
//! `vendor/autoload.php`. The bundled stubs, `ClassLoader.php` and
//! `InstalledVersions.php`, are copied in beside them with the LICENSE that covers them.

use crate::builder::AutoloadMaps;
use crate::render;

use std::fs::create_dir_all;
use std::path::{Path, PathBuf};

use phast_core::platform::PlatformRequirements;

use anyhow::{Context, Result};

/// Writes all generated PHP files for a given vendor directory.
pub struct Writer {
    /// Root directory under which each package lives as `<vendor>/<name>`.
    vendor_dir: PathBuf,
    /// The `vendor/phast/` directory where generated files are placed.
    phast_dir: PathBuf,
    /// Suffix appended to the generated bootstrap class names to keep them
    /// unique. Derived from the lock content, so identical locks yield identical
    /// class names on every machine.
    suffix: String,
    /// When `true`, the generated bootstrap puts the loader in classmap-
    /// authoritative mode: the classmap is trusted exclusively and the PSR
    /// filesystem fallback is skipped. Off by default (optimize-only).
    authoritative: bool,
    /// Platform requirements the `platform_check.php` guard verifies. `None`
    /// leaves the always-generated guard inert (it performs no checks).
    platform_check: Option<PlatformRequirements>,
}

impl Writer {
    /// Create a writer over `vendor_dir`, suffixing generated class names with `suffix`.
    pub fn new(vendor_dir: &Path, suffix: &str) -> Self {
        Self {
            vendor_dir: vendor_dir.to_path_buf(),
            phast_dir: vendor_dir.join("phast"),
            suffix: suffix.to_owned(),
            authoritative: false,
            platform_check: None,
        }
    }

    /// Set classmap-authoritative mode on the generated bootstrap.
    pub fn authoritative(mut self, authoritative: bool) -> Self {
        self.authoritative = authoritative;
        self
    }

    /// Set the platform requirements the `platform_check.php` guard verifies.
    ///
    /// `None` leaves the guard inert.
    pub fn platform_check(mut self, requirements: Option<PlatformRequirements>) -> Self {
        self.platform_check = requirements;
        self
    }

    /// Write every generated file: the maps rendered from `maps`, the stubs, and the LICENSE.
    pub fn write(&self, maps: &AutoloadMaps) -> Result<()> {
        create_dir_all(&self.phast_dir).context("failed to create vendor/phast/")?;

        self.write_license()?;
        self.write_class_loader()?;
        self.write_installed_versions()?;
        self.write_psr4(maps)?;
        self.write_psr0(maps)?;
        self.write_classmap(maps)?;
        self.write_files(maps)?;
        self.write_autoload_static(maps)?;
        self.write_platform_check()?;
        self.write_autoload_real()?;
        self.write_autoload_entry()?;

        Ok(())
    }

    /// Write `vendor/phast/LICENSE` - Phast's dual MIT/Apache-2.0 notice,
    /// including the MIT attribution for the bundled ClassLoader.
    #[rustfmt::skip]
    fn write_license(&self) -> Result<()> {
        let content = include_str!("stubs/LICENSE");
        std::fs::write(self.phast_dir.join("LICENSE"), content)
            .context("failed to write LICENSE")
    }

    /// Write `vendor/phast/ClassLoader.php` -- Phast's own PSR-4/PSR-0/classmap loader.
    fn write_class_loader(&self) -> Result<()> {
        let content = include_str!("stubs/ClassLoader.php");
        std::fs::write(self.phast_dir.join("ClassLoader.php"), content)
            .context("failed to write ClassLoader.php")
    }

    /// Write `vendor/phast/InstalledVersions.php` -- the `\Phast\InstalledVersions` shim.
    fn write_installed_versions(&self) -> Result<()> {
        let content = include_str!("stubs/InstalledVersions.php");
        std::fs::write(self.phast_dir.join("InstalledVersions.php"), content)
            .context("failed to write InstalledVersions.php")
    }

    /// Write `vendor/phast/autoload_psr4.php`.
    fn write_psr4(&self, maps: &AutoloadMaps) -> Result<()> {
        write_array_file(
            &self.phast_dir.join("autoload_psr4.php"),
            render::string_array_map(&maps.psr4),
        )
    }

    /// Write `vendor/phast/autoload_namespaces.php` (PSR-0).
    fn write_psr0(&self, maps: &AutoloadMaps) -> Result<()> {
        write_array_file(
            &self.phast_dir.join("autoload_namespaces.php"),
            render::string_array_map(&maps.psr0),
        )
    }

    /// Write `vendor/phast/autoload_classmap.php`.
    fn write_classmap(&self, maps: &AutoloadMaps) -> Result<()> {
        write_array_file(
            &self.phast_dir.join("autoload_classmap.php"),
            render::flat_string_map(&maps.classmap),
        )
    }

    /// Write `vendor/phast/autoload_files.php`.
    fn write_files(&self, maps: &AutoloadMaps) -> Result<()> {
        write_array_file(
            &self.phast_dir.join("autoload_files.php"),
            render::flat_ordered_map(&maps.files),
        )
    }

    /// Write `vendor/phast/autoload_static.php` -- combined static maps for fast startup.
    fn write_autoload_static(&self, maps: &AutoloadMaps) -> Result<()> {
        let content = include_str!("stubs/autoload_static.php")
            .replace("{class_name}", &format!("PhastStaticInit{}", self.suffix))
            .replace("{files}", &render::flat_ordered_map(&maps.files))
            .replace("{classmap}", &render::flat_string_map(&maps.classmap))
            .replace("{psr4}", &render::string_array_map(&maps.psr4))
            .replace("{psr0}", &render::string_array_map(&maps.psr0));

        std::fs::write(self.phast_dir.join("autoload_static.php"), content)
            .context("failed to write autoload_static.php")
    }

    /// Write `vendor/phast/platform_check.php` -- the fail-fast platform guard.
    ///
    /// Always written and always required by the bootstrap; when there is
    /// nothing to verify the checks are empty and the guard is inert.
    fn write_platform_check(&self) -> Result<()> {
        let checks = issue_checks(self.platform_check.as_ref());
        let content = include_str!("stubs/platform_check.php").replace("{checks}", &checks);

        std::fs::write(self.phast_dir.join("platform_check.php"), content)
            .context("failed to write platform_check.php")
    }

    /// Write `vendor/phast/autoload_real.php` -- the bootstrap class.
    fn write_autoload_real(&self) -> Result<()> {
        let authoritative = if self.authoritative { "true" } else { "false" };

        let content = include_str!("stubs/autoload_real.php")
            .replace(
                "{init_class}",
                &format!("PhastAutoloaderInit{}", self.suffix),
            )
            .replace("{static_class}", &format!("PhastStaticInit{}", self.suffix))
            .replace("{require_fn}", &format!("phastRequire{}", self.suffix))
            .replace("{authoritative}", authoritative);

        std::fs::write(self.phast_dir.join("autoload_real.php"), content)
            .context("failed to write autoload_real.php")
    }

    /// Write `vendor/autoload.php` -- the entry point every PHP app requires.
    fn write_autoload_entry(&self) -> Result<()> {
        let content = include_str!("stubs/autoload_entry.php").replace(
            "{init_class}",
            &format!("PhastAutoloaderInit{}", self.suffix),
        );

        std::fs::write(self.vendor_dir.join("autoload.php"), content)
            .context("failed to write vendor/autoload.php")
    }
}

/// Write a PHP file that returns a single array expression.
#[rustfmt::skip]
fn write_array_file(path: &Path, body: String) -> Result<()> {
    let content = format!(
        "<?php\n\n// This file is @generated by Phast. Do not edit.\n\nreturn {};\n",
        body,
    );

    let filename = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    std::fs::write(path, content)
        .with_context(|| format!("failed to write {}", filename))
}

/// Render the `platform_check.php` guard's checks from `requirements` as the PHP
/// `array(...)` passed to `array_filter`: a php-version check, plus one
/// `extension_loaded` check per required extension. `array()` when there is
/// nothing to verify, leaving the guard inert.
fn issue_checks(requirements: Option<&PlatformRequirements>) -> String {
    let Some(requirements) = requirements else {
        return render::array(&[]);
    };

    let mut checks: Vec<String> = Vec::new();

    if let Some(id) = requirements.php {
        // PHP packs a version into `PHP_VERSION_ID` as
        //      major*10000 + minor*100 + patch (8.1.0 -> 80100).
        //
        // Unpack it back into its parts for a human-readable ">= x.y.z" in the message;
        // the raw id drives the check.
        let (major, minor, patch) = (id / 10000, id / 100 % 100, id % 100);
        checks.push(issue_unless(
            &format!("PHP_VERSION_ID >= {id}"),
            &format!(
                "'Your dependencies require a PHP version \">= {major}.{minor}.{patch}\". \
                 You are running ' . PHP_VERSION . '.'"
            ),
        ));
    }

    for extension in &requirements.extensions {
        checks.push(issue_unless(
            &format!("extension_loaded('{extension}')"),
            &format!("'Your dependencies require the \"{extension}\" PHP extension.'"),
        ));
    }
    render::array(&checks)
}

/// One `array_filter` element: the message unless the condition holds,
/// i.e. `(<condition>) ? null : <message>`.
fn issue_unless(condition: &str, message: &str) -> String {
    format!("    ({condition}) ? null : {message},")
}
