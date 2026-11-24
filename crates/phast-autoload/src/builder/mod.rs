//! Autoload map builder.
//!
//! [`Builder`] gives you either the full maps or just the PSR rules. Both are built
//! from the same [`Sources`], so they never disagree about which packages are included.

mod maps;
mod rules;
mod sources;

pub use maps::AutoloadMaps;
pub use rules::AutoloadRules;
pub use sources::{Collect, Sources};

use maps::Maps;
use rules::Rules;

use std::path::{Path, PathBuf};

use phast_core::lock::phast::PhastLock;
use phast_workspace::MemberAutoload;

use anyhow::Result;

/// Builds autoload maps or PSR rules from a lock file.
pub struct Builder {
    /// Root directory under which each package lives as `<vendor>/<name>`.
    vendor_dir: PathBuf,
    /// Whether to include dev packages and dev autoload rules.
    include_dev: bool,
}

impl Builder {
    /// Create a builder for the packages in `vendor_dir`.
    pub fn new(vendor_dir: &Path, include_dev: bool) -> Self {
        Self {
            vendor_dir: vendor_dir.to_path_buf(),
            include_dev,
        }
    }

    /// The full autoload maps for `lock` and the workspace `members`.
    #[rustfmt::skip]
    pub fn maps(&self, lock: &PhastLock, members: &[MemberAutoload]) -> Result<AutoloadMaps> {
        self.collect(lock, members, Maps::new(&self.vendor_dir))
    }

    /// Just the PSR-4 and PSR-0 rules. Faster than `maps`, since nothing is scanned.
    #[rustfmt::skip]
    pub fn rules(&self, lock: &PhastLock, members: &[MemberAutoload]) -> Result<AutoloadRules> {
        self.collect(lock, members, Rules::new())
    }

    /// Feed every source to `collector`, then finish it.
    fn collect<C: Collect>(
        &self,
        lock: &PhastLock,
        members: &[MemberAutoload],
        collector: C,
    ) -> Result<C::Output> {
        Sources::new(&self.vendor_dir, self.include_dev, collector).collect(lock, members)
    }
}
