//! PSR-4 and PSR-0 autoload rules.

use super::sources::Collect;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use phast_core::lock::phast::AutoloadSection;

use anyhow::Result;

/// Fills [`AutoloadRules`].
pub(super) struct Rules {
    /// The rules collected so far.
    autoload_rules: AutoloadRules,
}

impl Rules {
    /// Empty rules, ready for the first source.
    pub(super) fn new() -> Self {
        Self {
            autoload_rules: AutoloadRules::default(),
        }
    }

    /// Add the PSR-4 rules from `autoload`. Its paths are relative to `dir`.
    fn collect_psr4(&mut self, autoload: &AutoloadSection, dir: &Path) {
        for (prefix, paths) in &autoload.psr4 {
            for path in paths {
                self.autoload_rules
                    .psr4
                    .entry(prefix.clone())
                    .or_default()
                    .push(dir.join(path));
            }
        }
    }

    /// Add the PSR-0 rules from `autoload`. Its paths are relative to `dir`.
    fn collect_psr0(&mut self, autoload: &AutoloadSection, dir: &Path) {
        for (prefix, paths) in &autoload.psr0 {
            for path in paths {
                self.autoload_rules
                    .psr0
                    .entry(prefix.clone())
                    .or_default()
                    .push(dir.join(path));
            }
        }
    }
}

impl Collect for Rules {
    type Output = AutoloadRules;

    fn add(&mut self, autoload: &AutoloadSection, dir: &Path) -> Result<()> {
        self.collect_psr4(autoload, dir);
        self.collect_psr0(autoload, dir);
        Ok(())
    }

    fn finish(self) -> AutoloadRules {
        self.autoload_rules
    }
}

/// Namespace prefixes and the directories they load from, for PSR-4 and PSR-0.
///
/// Cheaper than [`AutoloadMaps`](crate::builder::AutoloadMaps), because nothing is scanned.
///
/// A directory is only as absolute as the source it came from. A workspace member's
/// directory can be relative, so its rules can be too.
#[derive(Debug, Default, PartialEq)]
pub struct AutoloadRules {
    /// PSR-4: namespace prefix -> its directories, in the order they were declared.
    pub psr4: BTreeMap<String, Vec<PathBuf>>,
    /// PSR-0: namespace prefix -> its directories, in the order they were declared.
    pub psr0: BTreeMap<String, Vec<PathBuf>>,
}
