//! PHP autoloader generation for Phast.
//!
//! Generates `vendor/autoload.php` and support files under `vendor/phast/`
//! from the contents of `phast.lock`. No Composer source is bundled.
//!
//! ## Generated layout
//!
//! ```text
//! vendor/
//!   autoload.php                <- entry point (require this in your app)
//!   phast/
//!     ClassLoader.php           <- \Phast\Autoload\ClassLoader implementation
//!     autoload_real.php         <- PhastAutoloaderInit{HASH}, registers loader
//!     autoload_static.php       <- static maps for fast startup
//!     autoload_psr4.php         <- PSR-4 prefix -> path map
//!     autoload_namespaces.php   <- PSR-0 prefix -> path map
//!     autoload_classmap.php     <- class -> file map
//!     autoload_files.php        <- files required unconditionally
//!     platform_check.php        <- fail-fast php/extension guard
//!     InstalledVersions.php     <- \Phast\InstalledVersions runtime API
//!     installed.php             <- installed package metadata
//! ```
//!
//! ## Composer compatibility
//!
//! Phast implements `\Phast\Autoload\ClassLoader` and `\Phast\InstalledVersions`
//! directly. Two `class_alias` calls in `autoload_real.php` make the Composer names
//! resolve to those classes, so a package that references `\Composer\Autoload\ClassLoader`
//! or `\Composer\InstalledVersions` keeps working unchanged.

pub mod bin;
pub mod builder;
pub mod installed;
pub mod render;
pub mod scanner;
pub mod writer;

use crate::builder::Builder;
use crate::installed::Installed;
use crate::scanner::Duplicate;
use crate::writer::Writer;

use std::path::Path;

use phast_core::lock::phast::PhastLock;
use phast_core::platform::PlatformRequirements;
use phast_workspace::Workspace;

use anyhow::Result;
use sha2::{Digest, Sha256};

/// Options controlling how the autoloader is generated.
#[derive(Debug, Clone, Copy, Default)]
pub struct AutoloadOptions {
    /// When `true`, dev packages contribute to the generated maps.
    pub include_dev: bool,
    /// When `true`, generate the loader in classmap-authoritative mode: the
    /// classmap is trusted exclusively and the PSR filesystem fallback is
    /// dropped. For immutable production trees.
    pub authoritative: bool,
}

/// What [`generate`] observed while building the autoloader.
///
/// Diagnostics only, and never a reason to fail: every field describes something
/// worth telling the user about a tree that was still generated successfully.
/// A caller that ignores the report gets a working autoloader.
#[derive(Debug, Default)]
pub struct AutoloadReport {
    /// Classes declared in more than one scanned file. The classmap keeps the first
    /// and ignores the rest, so this is the caller's chance to say which lost.
    pub duplicates: Vec<Duplicate>,
}

/// Generate the full autoloader for the given lock into `vendor_dir`.
///
/// Always regenerates all files to ensure they reflect the current lock.
///
/// Member source trees from `workspace` are registered so their classes
/// are accessible without being copied into `vendor/`.
///
/// Returns an [`AutoloadReport`] of what was worth noticing along the way; see its
/// documentation for why that is separate from the `Result`.
#[rustfmt::skip]
pub fn generate(
    lock: &PhastLock,
    vendor_dir: &Path,
    options: AutoloadOptions,
    workspace: &Workspace,
) -> Result<AutoloadReport> {
    let members = workspace
        .members()
        .iter()
        .map(|member| member.autoloads())
        .collect::<Result<Vec<_>>>()?;

    // Suffix the bootstrap class names with an 8-char hash of the lock's
    // content-hash. That hash is a property of the locked inputs, not of the
    // local filesystem, so the same lock yields the same class names on every
    // machine.
    let _bytes = lock.content_hash.as_bytes();
    let suffix = hex::encode(&Sha256::digest(_bytes)[..4]);

    // Aggregate the php/extension floor the platform_check.php guard verifies,
    // across the root manifest and the whole locked tree. `None` when the
    // manifest disables the check, leaving the guard inert.
    let requirements = PlatformRequirements::collect(
        lock,
        workspace.manifest(),
        options.include_dev,
    );

    let autoload_maps = Builder::new(vendor_dir, options.include_dev)
        .build(lock, &members)?;
    Writer::new(vendor_dir, &suffix)
        .authoritative(options.authoritative)
        .platform_check(requirements)
        .write(&autoload_maps)?;
    Installed::new(vendor_dir, options.include_dev)
        .write(lock)?;

    Ok(AutoloadReport {
        duplicates: autoload_maps.duplicates,
    })
}
