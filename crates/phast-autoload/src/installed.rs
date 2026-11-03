//! `vendor/phast/installed.php` and `vendor/composer/installed.json` generation.
//!
//! Many packages call `\Composer\InstalledVersions::getVersion('vendor/pkg')`
//! at runtime to check what is installed. This call reads `installed.php`.
//! Without it, those packages throw or silently fail.
//!
//! `vendor/composer/installed.json` is read by Laravel's `PackageManifest::build()`
//! to discover service providers via `extra.laravel`. Without it, `package:discover`
//! writes an empty `bootstrap/cache/packages.php` and providers are never registered.

use crate::render;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use phast_core::lock::phast::{PackageDef, PhastLock};

use anyhow::{Context, Result};
use serde_json::{Value, json};

/// One entry of the `versions` map in `installed.php`, with `{placeholder}` slots
/// filled by [`Installed::render_php_version`].
const PHP_VERSION_ENTRY: &str = r#"
        {name} => array(
            'pretty_version' => {pretty_version},
            'version' => {version},
            'reference' => {reference},
            'type' => {type},
            'install_path' => {install_path},
            'aliases' => array(),
            'dev_requirement' => {dev_requirement},
        ),
"#;

/// Writes `vendor/phast/installed.php` and `vendor/composer/installed.json` from a lock file.
pub struct Installed {
    /// The `vendor/phast/` directory where `installed.php` is placed.
    phast_dir: PathBuf,
    /// The `vendor/composer/` directory where `installed.json` is placed.
    composer_dir: PathBuf,
    /// When `true`, dev packages are listed in the output.
    include_dev: bool,
}

#[rustfmt::skip]
impl Installed {
    /// Create a writer over `vendor_dir`, listing dev packages when `include_dev`.
    pub fn new(vendor_dir: &Path, include_dev: bool) -> Self {
        Self {
            phast_dir: vendor_dir.join("phast"),
            composer_dir: vendor_dir.join("composer"),
            include_dev,
        }
    }

    /// Write `vendor/phast/installed.php` and `vendor/composer/installed.json` from `lock`.
    pub fn write(&self, lock: &PhastLock) -> Result<()> {
        std::fs::create_dir_all(&self.phast_dir)
            .context("failed to create vendor/phast/")?;
        std::fs::write(self.phast_dir.join("installed.php"), self.render_php(lock))
            .context("failed to write installed.php")?;

        std::fs::create_dir_all(&self.composer_dir)
            .context("failed to create vendor/composer/")?;
        std::fs::write(self.composer_dir.join("installed.json"), self.render_json(lock))
            .context("failed to write installed.json")?;

        Ok(())
    }

    /// Render the full `installed.php` content for `lock`.
    fn render_php(&self, lock: &PhastLock) -> String {
        let packages = self
            .collect_packages(lock);
        let dev_package_names = self
            .collect_dev_package_names(lock);

        let versions: String = packages
            .iter()
            .map(|p| self.render_php_version(p, dev_package_names.contains(p.name.as_str())))
            .collect();

        let dev = if self.include_dev { "true" } else { "false" };

        include_str!("stubs/installed.php")
            .replace("{install_path}", "__DIR__ . '/../../'")
            .replace("{dev}", dev)
            .replace("{versions}", &versions)
    }

    /// Render a single entry in the `versions` map.
    fn render_php_version(&self, package: &PackageDef, dev_requirement: bool) -> String {
        let reference = reference(package)
            .map_or_else(|| "NULL".to_owned(), render::string);

        let package_type = package
            .package_type
            .as_deref()
            .unwrap_or("library");

        let dev_requirement = if dev_requirement { "true" } else { "false" };

        PHP_VERSION_ENTRY
            .trim_start_matches('\n')
            .replace("{name}", &render::string(&package.name))
            .replace("{pretty_version}", &render::string(&package.version))
            .replace("{version}", &render::string(&package.version))
            .replace("{reference}", &reference)
            .replace("{type}", &render::string(package_type))
            .replace("{install_path}", &format!("__DIR__ . '/../{}/'", package.name))
            .replace("{dev_requirement}", dev_requirement)
    }

    /// Render the full `installed.json` content for `lock`.
    fn render_json(&self, lock: &PhastLock) -> String {
        let packages = self
            .collect_packages(lock);
        let dev_package_names = self
            .collect_dev_package_names(lock);

        let root = json!({
            "name": "__root__",
            "pretty_version": "dev-main",
            "version": "dev-main",
            "reference": Value::Null,
            "type": "project",
            "install-path": "../../",
            "aliases": [],
            "dev": self.include_dev,
        });

        let mut all_packages = vec![root];
        all_packages.extend(
            packages.iter().map(|p| {
                self.render_json_package(p, dev_package_names.contains(p.name.as_str()))
            })
        );

        // Sorted because the set behind it is a `HashSet`, whose order is randomized
        // per instance. Without this the array comes out differently on every run and
        // an unchanged lock rewrites installed.json.
        let mut dev_package_name_list: Vec<&str> = dev_package_names
            .iter()
            .copied()
            .collect();
        dev_package_name_list.sort_unstable();

        let json = json!({
            "packages": all_packages,
            "dev": self.include_dev,
            "dev-package-names": dev_package_name_list,
        });

        serde_json::to_string_pretty(&json)
            .unwrap_or_default()
    }

    /// Build a JSON object for a single package entry in `installed.json`.
    ///
    /// Note the key and value conventions differ from `installed.php`, which is why
    /// the two renderers do not share one shape: this file spells the key `install-path`
    /// and states it relative to `vendor/composer/`, so a `vendor/` tree stays relocatable
    /// and the file does not embed the build machine's layout.
    fn render_json_package(&self, package: &PackageDef, dev_requirement: bool) -> Value {
        let reference: Value = reference(package)
            .map_or(Value::Null, |r| json!(r));

        let package_type = package
            .package_type
            .as_deref()
            .unwrap_or("library");

        let extra = package.extra.clone().map(|e| e.data).unwrap_or_default();

        json!({
            "name": package.name,
            "pretty_version": package.version,
            "version": package.version,
            "version_normalized": package.version,
            "reference": reference,
            "type": package_type,
            "install-path": format!("../{}", package.name),
            "aliases": [],
            "dev_requirement": dev_requirement,
            "extra": extra,
        })
    }

    /// The packages to list: production entries, plus dev ones when `include_dev`.
    fn collect_packages<'a>(&self, lock: &'a PhastLock) -> Vec<&'a PackageDef> {
        let mut packages: Vec<&PackageDef> = lock.packages
            .iter()
            .collect();
        if self.include_dev {
            packages.extend(lock.packages_dev.iter());
        }
        packages
    }

    /// The names of every dev package, for tagging entries as `dev_requirement`.
    fn collect_dev_package_names<'a>(&self, lock: &'a PhastLock) -> HashSet<&'a str> {
        lock.packages_dev
            .iter()
            .map(|p| p.name.as_str())
            .collect()
    }
}

/// The commit reference to record for `package`: the dist's, falling back to the
/// source's, and `None` when neither carries one (a path or metapackage entry).
fn reference(package: &PackageDef) -> Option<&str> {
    package
        .dist
        .as_ref()
        .and_then(|d| d.reference.as_deref())
        .or_else(|| package.source.as_ref().map(|s| s.reference.as_str()))
}
