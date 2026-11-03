//! PHP class scanner for classmap autoloading.
//!
//! Three layers, one job each:
//!
//! - `lexer` turns PHP source into a stream of tokens, preserving the context
//!   (`$var`, `::`, `->`, heredocs, comments) that distinguishes a real declaration
//!   from a look-alike.
//! - `declarations` interprets that stream, returning the fully-qualified names
//!   of the classes/interfaces/traits/enums a file declares.
//! - [`Scanner`] walks files and directories, resolving each declared name
//!   to its file path and accumulating the classmap.
//!
//! Used when a package declares `"autoload": {"classmap": ["src/"]}`, and when
//! scanning PSR-4 / PSR-0 roots so every known class resolves with one hash lookup.

mod declarations;
mod exclude;
mod lexer;

pub use declarations::extract;
pub use exclude::Exclude;

use std::collections::hash_map::{Entry, IntoIter};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

/// Accumulates a classmap (FQCN -> absolute file path) by scanning PHP files.
///
/// Call [`scan_path`](Scanner::scan_path) one or more times, then consume the
/// scanner via [`IntoIterator`] to drain the collected entries into a target map.
///
/// ```
/// use phast_autoload::scanner::Scanner;
/// use std::collections::HashMap;
///
/// let dir = tempfile::tempdir()?;
/// std::fs::write(dir.path().join("Widget.php"), "<?php namespace Acme; class Widget {}")?;
///
/// let mut scanner = Scanner::new(None);
/// scanner.scan_path(dir.path())?;
///
/// let classmap: HashMap<String, String> = scanner.into_iter().collect();
/// assert!(classmap.contains_key("Acme\\Widget"));
/// # Ok::<(), anyhow::Error>(())
/// ```
#[derive(Debug, Default)]
pub struct Scanner {
    /// Fully-qualified class name -> absolute file path.
    map: HashMap<String, String>,
    /// Classes found in more than one file. See [`Duplicate`] for which one wins.
    duplicates: Vec<Duplicate>,
    /// Paths already scanned in this session, so a file reached through
    /// overlapping scan roots (a directory plus a file nested inside it) is
    /// scanned once instead of reporting itself as a duplicate.
    visited: HashSet<String>,
    /// Files matching this matcher are skipped while scanning, so their classes
    /// never enter the classmap. `None` when the package excludes nothing.
    exclude: Option<Arc<Exclude>>,
}

impl Scanner {
    /// Create a scanner. Files matched by `exclude` are skipped while walking a
    /// tree, so their classes never enter the classmap; pass `None` to scan
    /// everything.
    pub fn new(exclude: Option<Arc<Exclude>>) -> Self {
        Self {
            exclude,
            ..Self::default()
        }
    }

    /// The duplicate class declarations found so far: a class seen in more
    /// than one file, with the kept (first) and ignored (later) paths.
    pub fn duplicates(&self) -> &[Duplicate] {
        &self.duplicates
    }

    /// Scan `path` (file or directory) and accumulate discovered FQCNs.
    ///
    /// Returns an error if `path` does not exist or is not a regular file or directory.
    /// Callers should ensure packages are extracted before calling this.
    pub fn scan_path(&mut self, path: &Path) -> anyhow::Result<()> {
        if path.is_dir() {
            return self.scan_dir(path);
        }
        if path.is_file() {
            return self.scan_file(path);
        }

        anyhow::bail!(
            "classmap path does not exist or is not a file or directory: {}",
            path.display()
        );
    }

    /// Recursively scan a directory, visiting all `.php` and `.inc` files.
    fn scan_dir(&mut self, dir: &Path) -> anyhow::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                self.scan_dir(&path)?;
            }
            if is_php_file(&path) {
                self.scan_file(&path)?;
            }
        }
        Ok(())
    }

    /// Scan a single PHP file and accumulate any discovered FQCNs.
    fn scan_file(&mut self, path: &Path) -> anyhow::Result<()> {
        // A file the package excludes from its classmap is never scanned, so its
        // classes stay out of the map entirely (Composer's dump-time semantics).
        if let Some(exclude) = &self.exclude
            && exclude.excludes(path)
        {
            return Ok(());
        };

        // Scan each file at most once. Packages sometimes list a directory and a
        // file nested inside it in the same `classmap`; without this, the file is
        // walked twice and the second visit reports a spurious self-duplicate.
        let abs = path.to_string_lossy().into_owned();
        if !self.visited.insert(abs.clone()) {
            return Ok(());
        }

        // Use lossy UTF-8 conversion: some PHP files in the wild contain ISO-8859-1
        // or other non-UTF-8 bytes in string literals or comments.
        //
        // The replacement character U+FFFD is harmless for our lexer;
        // we only care about ASCII keywords and identifiers.
        let _bytes = std::fs::read(path)?;
        let source = String::from_utf8_lossy(&_bytes);

        for fqcn in extract(&source) {
            let duplicate = match self.map.entry(fqcn) {
                // First time we see this class: record its file.
                Entry::Vacant(slot) => {
                    slot.insert(abs.clone());
                    None
                }
                // Seen before: keep the earlier file, report this one as a duplicate.
                Entry::Occupied(existing) => Some(Duplicate {
                    class: existing.key().clone(),
                    kept: existing.get().clone(),
                    ignored: abs.clone(),
                }),
            };
            if let Some(duplicate) = duplicate {
                self.duplicates.push(duplicate);
            }
        }
        Ok(())
    }
}

/// The collected entries, drained. Their order is unspecified, since they live in a
/// [`HashMap`]: a caller that needs a stable sequence has to sort them itself.
impl IntoIterator for Scanner {
    type Item = (String, String);
    type IntoIter = IntoIter<String, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

/// A class declared in more than one scanned file. The first definition wins;
/// each later one is dropped and recorded here so the caller can warn.
#[derive(Debug, Clone)]
pub struct Duplicate {
    /// The fully-qualified class name found more than once.
    pub class: String,
    /// Path of the kept definition (the first one seen).
    pub kept: String,
    /// Path of the ignored definition (a later one).
    pub ignored: String,
}

/// Whether `path` has a `.php` or `.inc` extension.
fn is_php_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("php") | Some("inc"),
    )
}
