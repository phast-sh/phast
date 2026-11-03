//! Scaffolding wizard for `phast init`.
//!
//! [`InitOptions`] carries the answers supplied up front, plus whether the wizard may
//! ask for the rest. Every field resolves the same way: the supplied answer, then the
//! prompt when interactive, then the default. One resolver per field keeps the
//! prompted and unprompted paths reading the same default.

use crate::hints;
use crate::output::Printer;
use crate::platform;
use crate::styles;

use std::io::Write;
use std::path::Path;

use phast_core::manifest::phast::{AutoloadSection, ConfigSection, PackageSection, RequireSection};
use phast_core::manifest::phast::{DependencySpec, PhastManifest, Platform};
use phast_core::{PackageName, PackageType};
use phast_version::Version;

use anyhow::{Context, Result, bail};
use dialoguer::{Confirm, Select};
use indexmap::IndexMap;

/// Collects package metadata and scaffolds the directory layout.
///
/// Asks for whatever [`InitOptions`] left open, and only when it may. Returns a fully
/// populated `PhastManifest` for the caller to serialize and write to disk.
pub struct Wizard<'a> {
    options: InitOptions,
    printer: &'a Printer,
}

impl<'a> Wizard<'a> {
    pub fn new(options: InitOptions, printer: &'a Printer) -> Self {
        Self { options, printer }
    }

    /// Resolve every field and scaffold the package directory.
    ///
    /// Creates `target_dir` if it does not exist, aborts if `phast.toml` is already
    /// present, and creates the optional `src/` and `tests/` subdirectories. Returns
    /// the populated manifest -- the caller is responsible for serializing and
    /// writing `phast.toml`.
    #[rustfmt::skip]
    pub fn scaffold(&self, target_dir: &Path) -> Result<PhastManifest> {
        std::fs::create_dir_all(target_dir)
            .with_context(|| format!("failed to create directory '{}'", target_dir.display()))?;

        if target_dir.join("phast.toml").exists() {
            bail!(hints::phast_toml_exists());
        }

        let name = self.package_name(target_dir)?;
        let namespace = derive_namespace(name.as_str());

        let inputs = InitInputs {
            package_name: name,
            package_type: self.package_type()?,
            description:  self.description()?,
            php_version:  self.php_version()?,
            with_src:     self.directory("Create src/ directory",   self.options.with_src)?,
            with_tests:   self.directory("Create tests/ directory", self.options.with_tests)?,
            namespace,
        };

        if inputs.with_src {
            std::fs::create_dir_all(target_dir.join("src"))
                .context("failed to create src/")?;
        }
        if inputs.with_tests {
            std::fs::create_dir_all(target_dir.join("tests"))
                .context("failed to create tests/")?;
        }

        Ok(inputs.into())
    }

    /// Print `label [default]:` (or `label:` when no default is given), read one line from stdin,
    /// and return the trimmed input -- or `default` when the user presses Enter without typing.
    #[rustfmt::skip]
    fn prompt(&self, label: &str, default: Option<&str>) -> Result<String> {
        match default {
            Some(v) => print!("{} [{}]: ", label, styles::DEFAULT_KEY.apply_to(v)),
            None    => print!("{}: ", label),
        }
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let input = input.trim();

        Ok(if input.is_empty() {
            default.unwrap_or("").to_string()
        } else {
            input.to_string()
        })
    }

    /// Show a yes/no prompt using `dialoguer::Confirm`. Returns `default` when the user hits Enter.
    fn confirm(&self, label: &str, default: bool) -> Result<bool> {
        Ok(Confirm::new()
            .with_prompt(label)
            .default(default)
            .interact()?)
    }

    /// The package name: the supplied one, else a prompt, else `$USER/<directory>`.
    fn package_name(&self, target: &Path) -> Result<PackageName> {
        if let Some(package_name) = &self.options.package_name {
            return Ok(package_name.clone());
        }

        let default = default_package_name(target);
        let entered = match self.options.interactive {
            true => self.prompt("Package name", Some(&default))?,
            false => default,
        };

        Ok(PackageName::new(entered))
    }

    /// The package type: the supplied one, else a menu, else [`DEFAULT_PACKAGE_TYPE`].
    #[rustfmt::skip]
    fn package_type(&self) -> Result<PackageType> {
        if let Some(package_type) = self.options.package_type {
            return Ok(package_type);
        }
        if !self.options.interactive {
            return Ok(DEFAULT_PACKAGE_TYPE);
        }

        let options = PackageType::all();
        let labels: Vec<&str> = options
            .iter()
            .map(|t| t.as_str())
            .collect();

        let default = options
            .iter()
            .position(|t| *t == DEFAULT_PACKAGE_TYPE)
            .unwrap_or(0);

        let index = Select::new()
            .with_prompt("Package type")
            .items(&labels)
            .default(default)
            .interact()?;

        Ok(options[index])
    }

    /// The description: the supplied one, else a prompt, else empty.
    ///
    /// An empty description is dropped when the manifest is assembled, so there is no
    /// difference between declining the prompt and never being asked.
    fn description(&self) -> Result<String> {
        if let Some(description) = &self.options.description {
            return Ok(description.clone());
        }
        if !self.options.interactive {
            return Ok(String::new());
        }

        self.prompt("Description", None)
    }

    /// The PHP constraint: the supplied one, else a prompt, else the detected runtime.
    ///
    /// This is the one field with no safe fallback. Interactively the prompt repeats
    /// until it gets an answer; without a terminal there is nothing to repeat, so an
    /// undetectable runtime fails here rather than guessing. A wrong constraint in
    /// `[require]` and `[config] platform` surfaces as a resolution failure much later,
    /// a long way from the manifest that caused it.
    fn php_version(&self) -> Result<String> {
        if let Some(php_version) = &self.options.php_version {
            return Ok(php_version.clone());
        }

        let detected = detect_php_constraint();
        if !self.options.interactive {
            let Some(detected) = detected else {
                bail!(hints::php_version_required());
            };
            return Ok(detected);
        }

        loop {
            let version = self.prompt("PHP version", detected.as_deref())?;
            if !version.is_empty() {
                return Ok(version);
            }
            self.printer.warning("PHP version is required, e.g. ^8.4");
        }
    }

    /// One of the optional directory questions: the supplied answer, else a prompt,
    /// else create it.
    fn directory(&self, label: &str, should_create: Option<bool>) -> Result<bool> {
        if let Some(decision) = should_create {
            return Ok(decision);
        }
        if !self.options.interactive {
            return Ok(true);
        }

        self.confirm(label, true)
    }
}

/// All inputs collected by the wizard before the manifest is assembled.
///
/// A new prompt lands here and in `From<InitInputs>`, but also needs a resolver on
/// [`Wizard`] and, since every prompted field has a flag, a field on [`InitOptions`]
/// and an argument on `InitArgs`.
struct InitInputs {
    package_name: PackageName,
    package_type: PackageType,
    description: String,
    php_version: String,
    namespace: String,
    with_src: bool,
    with_tests: bool,
}

#[rustfmt::skip]
impl From<InitInputs> for PhastManifest {
    fn from(inputs: InitInputs) -> Self {
        let package_name = Some(inputs.package_name.to_string());
        let package_type = Some(inputs.package_type.as_str().to_string());
        let platform_php = Some(inputs.php_version.trim_start_matches('^').to_string());

        let mut description = None;
        if !inputs.description.is_empty() {
            description = Some(inputs.description);
        }

        let package = PackageSection {
            name: package_name,
            description,
            package_type,
            ..Default::default()
        };

        let mut dependencies = IndexMap::new();
        dependencies.insert(
            "php".to_string(),
            DependencySpec::Version(inputs.php_version.clone()),
        );
        let require = RequireSection { dependencies };

        let mut autoload = AutoloadSection::default();
        if inputs.with_src {
            let mut psr4 = IndexMap::new();
            psr4.insert(format!("{}\\", inputs.namespace), vec!["src/"].into());
            autoload = AutoloadSection { psr4, ..Default::default() }
        }

        let mut autoload_dev = AutoloadSection::default();
        if inputs.with_tests {
            let mut psr4 = IndexMap::new();
            psr4.insert("Tests\\".to_string(), vec!["tests/"].into());
            autoload_dev = AutoloadSection { psr4, ..Default::default() }
        }

        let config = ConfigSection {
            platform: Platform {
                php: platform_php,
                ..Default::default()
            },
            ..Default::default()
        };

        PhastManifest {
            package,
            require,
            autoload,
            autoload_dev,
            config,
            ..Default::default()
        }
    }
}

/// The package type used when nothing else decides, on either path.
const DEFAULT_PACKAGE_TYPE: PackageType = PackageType::Library;

/// Answers supplied before the wizard runs, and whether it may ask for the rest.
///
/// A `Some` field is already decided, so its prompt is skipped. A `None` field is
/// asked for when [`interactive`](Self::interactive), and takes its default when not.
#[derive(Default)]
pub struct InitOptions {
    /// The `vendor/package` name.
    pub package_name: Option<PackageName>,
    /// The package type written to `[package] type`.
    pub package_type: Option<PackageType>,
    /// The description; an empty string is omitted from the manifest.
    pub description: Option<String>,
    /// The PHP version constraint, e.g. `^8.4`.
    pub php_version: Option<String>,
    /// Whether to create `src/`.
    pub with_src: Option<bool>,
    /// Whether to create `tests/`.
    pub with_tests: Option<bool>,
    /// Whether the remaining questions may be asked.
    pub interactive: bool,
}

/// Derive a suggested package name from the system username and the target directory name.
fn default_package_name(target: &Path) -> String {
    let dir_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("package");

    let username = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "vendor".to_string());

    format!("{}/{}", username, dir_name)
}

/// The `^major.minor` constraint for the PHP runtime on PATH, when one is detectable.
fn detect_php_constraint() -> Option<String> {
    match platform::detect_php_version() {
        Ok(Version::Numeric(version)) => {
            Some(format!("^{}.{}", version.segments[0], version.segments[1]))
        }
        _ => None,
    }
}

/// Derive a PSR-4 namespace from a `vendor/package` name.
///
/// Each slash-separated component is PascalCased by splitting on the separators a
/// package name may contain. For example, `acme/my-package` produces `Acme\MyPackage`.
fn derive_namespace(package_name: &str) -> String {
    package_name
        .split('/')
        .map(pascal_case)
        .collect::<Vec<_>>()
        .join("\\")
}

/// Convert a separated string to PascalCase: `my-pkg` -> `MyPkg`.
///
/// Dots are separators too, not just hyphens and underscores. A package name may
/// legally contain them, but a PHP identifier may not, so leaving one in place
/// produces a namespace no class can ever match.
#[rustfmt::skip]
fn pascal_case(package_name: &str) -> String {
    package_name
        .split(['-', '_', '.'])
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None    => String::new(),
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            }
        })
        .collect()
}
