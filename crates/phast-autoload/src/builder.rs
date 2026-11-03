//! Autoload map builder.
//!
//! Walks `phast.lock` and collects PSR-4, PSR-0, classmap, and files entries
//! from every package into a single `AutoloadMaps` struct that the writer
//! can render to PHP without any further I/O.

use crate::scanner::{Duplicate, Exclude, Scanner};

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use phast_core::lock::phast::{AutoloadSection, PackageDef, PhastLock};
use phast_workspace::MemberAutoload;

use anyhow::Result;
use indexmap::IndexMap;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use sha2::{Digest, Sha256};

/// Builds [`AutoloadMaps`] from a lock file.
///
/// Everything is configured up front by [`Builder::new`];
/// the builder is then consumed by [`build`](Builder::build), which returns the collected maps.
pub struct Builder {
    /// Root directory under which each package lives as `<vendor>/<name>`.
    vendor_dir: PathBuf,
    /// When `true`, dev packages contribute to the autoloader.
    include_dev: bool,
    /// Accumulates autoload entries as packages are processed.
    autoload_maps: AutoloadMaps,
    /// PSR-4 / PSR-0 roots collected from autoload rules, scanned into the
    /// classmap at the end of [`build`](Builder::build) (optimize-by-default).
    psr_roots: Vec<PsrRoot>,
}

impl Builder {
    /// Create a builder over `vendor_dir`, folding in dev packages when `include_dev`.
    pub fn new(vendor_dir: &Path, include_dev: bool) -> Self {
        Self {
            vendor_dir: vendor_dir.to_path_buf(),
            include_dev,
            autoload_maps: AutoloadMaps::default(),
            psr_roots: Vec::new(),
        }
    }

    /// Build autoload maps from `lock` and the workspace `members`.
    ///
    /// Sources fold in dependency-topological order so a package's `files`
    /// autoloads run after everything it depends on:
    ///  - external packages first (the lock stores them dependencies-before-dependents)
    ///  - then workspace members
    ///  - then the root package last
    ///
    /// Each depends only on what came before it.
    #[rustfmt::skip]
    pub fn build(
        mut self,
        lock: &PhastLock,
        members: &[MemberAutoload],
    ) -> Result<AutoloadMaps> {
        // External packages first, in lock order (dependencies before dependents).
        // They carry production autoload rules only.
        let packages = self.installed_packages(lock);
        for package in packages {
            let dir = self.vendor_dir.join(&package.name);
            self.collect_section(&package.autoload, &dir)?;
        }

        // Workspace members next: they depend on the externals above.
        for member in members {
            self.collect_source(&member.dir, &member.autoload, &member.autoload_dev)?;
        }

        // The root package last: it depends on everything above.
        let root = self
            .vendor_dir
            .parent()
            .unwrap_or(&self.vendor_dir)
            .to_path_buf();
        self.collect_source(&root, &lock.autoload, &lock.autoload_dev)?;

        // Scan the collected PSR roots into the classmap, so at runtime every
        // known class resolves with one hash lookup instead of filesystem probes.
        self.collect_psr_roots();

        Ok(self.autoload_maps)
    }

    /// The lock's packages to process: production entries, plus dev entries when
    /// `include_dev` is set. Selection only; nothing is folded into the maps.
    fn installed_packages<'a>(&self, lock: &'a PhastLock) -> Vec<&'a PackageDef> {
        let mut packages = lock.packages.iter().collect::<Vec<_>>();
        if self.include_dev {
            packages.extend(lock.packages_dev.iter());
        }
        packages
    }

    /// Collect a source's production rules, plus its dev rules when `include_dev` is set.
    /// Used for the root and each workspace member.
    fn collect_source(
        &mut self,
        dir: &Path,
        autoload: &AutoloadSection,
        autoload_dev: &AutoloadSection,
    ) -> Result<()> {
        self.collect_section(autoload, dir)?;
        if self.include_dev {
            self.collect_section(autoload_dev, dir)?;
        }
        Ok(())
    }

    /// Fold one autoload section (PSR-4, PSR-0, classmap, files) rooted at `dir`.
    fn collect_section(&mut self, autoload: &AutoloadSection, dir: &Path) -> Result<()> {
        // The package's exclude patterns are resolved against its own root and
        // shared (by every PSR root and the classmap scan) for the whole section.
        let exclude = Exclude::new(dir, &autoload.exclude)?.map(Arc::new);
        self.collect_psr4(autoload, dir, &exclude);
        self.collect_psr0(autoload, dir, &exclude);
        self.collect_classmap(autoload, dir, &exclude)?;
        self.collect_files(autoload, dir);
        Ok(())
    }

    /// Collect PSR-4 autoload rules from `autoload`, rooted at `base_dir`.
    ///
    /// Each namespace prefix may map to multiple base directories across packages,
    /// so directories are appended rather than replaced.
    fn collect_psr4(
        &mut self,
        autoload: &AutoloadSection,
        base_dir: &Path,
        exclude: &Option<Arc<Exclude>>,
    ) {
        for (namespace, paths) in &autoload.psr4 {
            for path in paths {
                let dir = base_dir.join(path);
                self.autoload_maps
                    .psr4
                    .entry(namespace.clone())
                    .or_default()
                    .push(rel(&self.vendor_dir, dir.clone()));
                self.psr_roots.push(PsrRoot {
                    prefix: namespace.clone(),
                    dir,
                    kind: PsrKind::Psr4,
                    exclude: exclude.clone(),
                });
            }
        }
    }

    /// Collect PSR-0 autoload rules from `autoload`, rooted at `base_dir`.
    ///
    /// PSR-0 is the legacy predecessor to PSR-4: namespace separators and underscores
    /// in class names are both treated as directory separators during lookup.
    /// Like PSR-4, multiple packages can register the same prefix, so directories are appended.
    fn collect_psr0(
        &mut self,
        autoload: &AutoloadSection,
        base_dir: &Path,
        exclude: &Option<Arc<Exclude>>,
    ) {
        for (namespace, paths) in &autoload.psr0 {
            for path in paths {
                let dir = base_dir.join(path);
                self.autoload_maps
                    .psr0
                    .entry(namespace.clone())
                    .or_default()
                    .push(rel(&self.vendor_dir, dir.clone()));
                self.psr_roots.push(PsrRoot {
                    prefix: namespace.clone(),
                    dir,
                    kind: PsrKind::Psr0,
                    exclude: exclude.clone(),
                });
            }
        }
    }

    /// Collect classmap autoload rules from `autoload`, rooted at `base_dir`.
    ///
    /// Each entry in `autoload.classmap` is a path (file or directory) to scan
    /// for PHP class definitions. The scanner resolves fully-qualified class names
    /// to their absolute file paths, which are merged into the shared classmap.
    fn collect_classmap(
        &mut self,
        autoload: &AutoloadSection,
        base_dir: &Path,
        exclude: &Option<Arc<Exclude>>,
    ) -> Result<()> {
        let mut scanner = Scanner::new(exclude.clone());
        // Scan every declared classmap path (a file or a directory tree) for classes.
        for path in &autoload.classmap {
            scanner.scan_path(&base_dir.join(path))?;
        }

        // Carry the scanner's duplicate findings up as warnings, with vendor-relative
        // paths so the message matches how the classmap itself is stored.
        for duplicate in scanner.duplicates() {
            self.autoload_maps.duplicates.push(Duplicate {
                class: duplicate.class.clone(),
                kept: rel(&self.vendor_dir, PathBuf::from(&duplicate.kept)),
                ignored: rel(&self.vendor_dir, PathBuf::from(&duplicate.ignored)),
            });
        }

        // Merge the discovered classes into the shared map;
        // the first package to define a class keeps it.
        for (class, abs) in scanner {
            let rel = rel(&self.vendor_dir, PathBuf::from(&abs));
            self.autoload_maps.classmap.entry(class).or_insert(rel);
        }
        Ok(())
    }

    /// Collect files autoload entries from `autoload`, rooted at `base_dir`.
    ///
    /// Files entries are PHP scripts that must be required unconditionally at
    /// autoloader bootstrap time (e.g. global function libraries). Each file is
    /// stored under a stable key derived from its absolute path so that the same
    /// file is never required twice even if multiple packages declare it.
    fn collect_files(&mut self, autoload: &AutoloadSection, base_dir: &Path) {
        for file in &autoload.files {
            let abs = base_dir.join(file);
            let key = key(&abs.to_string_lossy());
            let rel = rel(&self.vendor_dir, abs);
            self.autoload_maps.files.entry(key).or_insert(rel);
        }
    }

    /// Scan every PSR root for class declarations and fold them into the classmap,
    /// so the runtime resolves a hash lookup instead of a filesystem probe.
    /// The PSR rules stay in place as a fallback for classes added after this dump.
    #[rustfmt::skip]
    fn collect_psr_roots(&mut self) {
        // Scan all roots in parallel; each filters by its own PSR kind.
        let per_root: Vec<Vec<(String, String)>> = self
            .psr_roots
            .par_iter()
            .map(PsrRoot::scan)
            .collect();

        // Merge in root order; declared classmap entries and the first root
        // to serve a class win (`or_insert`).
        for (class, file) in per_root.into_iter().flatten() {
            self.autoload_maps
                .classmap
                .entry(class)
                .or_insert_with(|| rel(&self.vendor_dir, PathBuf::from(file)));
        }
    }
}

/// All autoload maps collected from a lock file, ready to be written to disk.
#[derive(Debug, Default)]
pub struct AutoloadMaps {
    /// PSR-4: namespace prefix -> list of vendor-relative base directories.
    pub psr4: HashMap<String, Vec<String>>,
    /// PSR-0: namespace prefix -> list of vendor-relative base directories.
    pub psr0: HashMap<String, Vec<String>>,
    /// Classmap: fully-qualified class name -> vendor-relative file path.
    ///
    /// Sorted, so the rendered file is stable without the renderer having to sort.
    pub classmap: BTreeMap<String, String>,
    /// Files: stable key -> vendor-relative file path, in collection order.
    ///
    /// `IndexMap` preserves insertion order so the PHP autoloader requires
    /// files in the dependency-topological sequence [`Builder::build`] folds
    /// them in, keeping a package's files after those of its dependencies.
    pub files: IndexMap<String, String>,
    /// Duplicate class declarations found while scanning classmaps: the same class in
    /// more than one file. Diagnostic only; the caller surfaces these as warnings and
    /// nothing is written to a generated file.
    pub duplicates: Vec<Duplicate>,
}

/// A PSR autoload root: a namespace prefix and the base directory serving it.
struct PsrRoot {
    /// Namespace prefix with its trailing `\`, e.g. `Monolog\`.
    prefix: String,
    /// Absolute base directory.
    dir: PathBuf,
    /// Which PSR standard the root follows; decides how its scan is filtered.
    kind: PsrKind,
    /// The declaring package's classmap exclusions, shared across its roots.
    exclude: Option<Arc<Exclude>>,
}

/// The PSR autoloading standard a root follows.
enum PsrKind {
    /// PSR-4: only classes whose file is exactly where the prefix maps it are
    /// kept. A non-compliant class (a secondary class in a file, a misnamed
    /// file) could never resolve through the runtime PSR-4 lookup, so mapping
    /// it would make the classmap load classes the fallback cannot.
    Psr4,
    /// PSR-0: everything found is kept. A legacy layout whose path rules differ
    /// (underscores in the class name map to directories), so compliance
    /// filtering is not applied.
    Psr0,
}

impl PsrRoot {
    /// The classes this root serves, as `(class, absolute file)` pairs.
    /// A directory that is declared but absent on disk yields none.
    fn scan(&self) -> Vec<(String, String)> {
        let mut scanner = Scanner::new(self.exclude.clone());
        let _ = scanner.scan_path(&self.dir);

        scanner
            .into_iter()
            .filter(|(class, file)| self.keeps(class, file))
            .collect()
    }

    /// Whether this root's classmap keeps `class` declared in `file`.
    ///
    /// PSR-4 keeps only compliant classes: a `Helper` class declared alongside
    /// `Monolog\Logger` in `src/Logger.php` is scanned but not kept.
    /// PSR-0 keeps everything it finds.
    fn keeps(&self, class: &str, file: &str) -> bool {
        match self.kind {
            PsrKind::Psr0 => true,
            PsrKind::Psr4 => self.is_compliant(class, file),
        }
    }

    /// Whether `class` at `file` is where PSR-4 expects it for this root.
    ///
    /// Strip the prefix from the class name and the rest must spell the file
    /// path: for `Monolog\ -> src/`, `Monolog\Handler\Stream` must live in
    /// `src/Handler/Stream.php`.
    fn is_compliant(&self, class: &str, file: &str) -> bool {
        // The part of the class name below the prefix, e.g. `Sub\Name`.
        let Some(sub) = class.strip_prefix(&self.prefix) else {
            return false;
        };

        // Where PSR-4 says its file must be: `dir/Sub/Name.php`.
        let relative = format!("{}.php", sub.replace('\\', "/"));
        let expected = self.dir.join(relative);

        expected == Path::new(file)
    }
}

/// Stable identifier for a files entry: first 8 hex chars of SHA-256 of the path.
fn key(path: &str) -> String {
    let hash = Sha256::digest(path.as_bytes());
    hex::encode(&hash[..4])
}

/// Convert an absolute path to a path relative to `vendor_dir`.
///
/// Paths inside `vendor/` become `pkg/src/file.php`.
/// Paths under the project root (parent of `vendor/`) become `../app/file.php`.
/// Anything outside both falls back to the absolute path string.
#[rustfmt::skip]
fn rel(vendor_dir: &Path, abs: PathBuf) -> String {
    // Inside `vendor/`: strip it, giving `pkg/src/file.php`.
    if let Ok(path) = abs
        .strip_prefix(vendor_dir)
    {
        return path
            .to_string_lossy()
            .replace('\\', "/");
    }

    // Under the project root (parent of `vendor/`): strip it and prefix `../`.
    if let Some(Ok(path)) = vendor_dir
        .parent()
        .map(|p| abs.strip_prefix(p))
    {
        return format!("../{}", path
            .to_string_lossy()
            .replace('\\', "/"));
    }

    // Outside both: keep the absolute path.
    abs.to_string_lossy().into_owned()
}
