//! A single package inside a workspace.

use std::fmt;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use phast_core::lock::phast::AutoloadSection;
use phast_core::manifest::Manifest;
use phast_core::manifest::phast::PhastManifest;

use anyhow::{Context, Result};

/// A single workspace member: its location on disk plus its parsed manifest.
#[derive(Debug)]
pub struct Member {
    /// The member directory, as the workspace root joined with the matched glob.
    /// Absolute exactly when the root is, which is not guaranteed: a relative
    /// `--project-dir` produces a relative root and relative members with it.
    pub(crate) path: PathBuf,
    /// The full parsed manifest (requirements, autoload rules, etc.).
    pub(crate) manifest: PhastManifest,
}

impl Member {
    /// A member at `path`, with the manifest parsed from it.
    ///
    /// Discovery builds members while walking a workspace; this is for callers that
    /// assemble one directly.
    pub fn new(path: PathBuf, manifest: PhastManifest) -> Self {
        Self { path, manifest }
    }

    /// The member's directory, in whatever form the workspace root was given in.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The member's declared `[package].name`, if any.
    ///
    /// This is the member's identity: the key it appears under in `phast.lock` and the
    /// one a dependency closure resolves against. A member may omit it, in which case
    /// it has no package identity and no lock entry, which is why this is an `Option`
    /// rather than a name invented from the directory.
    ///
    /// For naming a member in a message, use its [`Display`] form instead.
    pub fn name(&self) -> Option<&str> {
        self.manifest.package.name.as_deref()
    }

    /// The member's declared `[package].version`, if any.
    ///
    /// `None` is not an error: a member may leave its version out. What to show or
    /// record instead is the caller's decision, and it genuinely varies. `phast.lock`
    /// substitutes `0.0.0` because the format needs a version.
    pub fn version(&self) -> Option<&str> {
        self.manifest.package.version.as_deref()
    }

    /// Derive normalized autoload configuration for this member.
    ///
    /// Paths are relative to [`Self::path`]; the autoload builder resolves them
    /// against that base when generating `vendor/autoload.php`.
    pub fn autoloads(&self) -> Result<MemberAutoload> {
        let manifest = self.to_manifest()?;

        Ok(MemberAutoload {
            dir: self.path.clone(),
            autoload: AutoloadSection::from(manifest.autoload),
            autoload_dev: AutoloadSection::from(manifest.autoload_dev),
        })
    }

    /// The member's manifest in the unified [`Manifest`] model.
    ///
    /// The error points at the member's `phast.toml` by path when it does not convert,
    /// which is why this exists rather than callers reaching for the raw manifest:
    /// getting that context right is easy to forget and easy to word differently.
    pub fn to_manifest(&self) -> Result<Manifest> {
        self.manifest
            .clone()
            .into_manifest()
            .with_context(|| format!("failed to parse member manifest at {}", self.path.display()))
    }
}

/// How a member is named in a message: its declared package name when it has one,
/// otherwise its directory, otherwise its full path.
///
/// Deliberately infallible and deliberately not an identity. Use [`Member::name`] when
/// the answer has to be the package this member actually is.
impl Display for Member {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // Prefer the declared package name from [package].name
        if let Some(name) = &self.manifest.package.name {
            return f.write_str(name);
        }
        // Fall back to the directory name when no package name is declared.
        if let Some(dir) = self.path.file_name() {
            return f.write_str(&dir.to_string_lossy());
        }
        // Last resort: full path.
        write!(f, "{}", self.path.display())
    }
}

/// Autoload configuration for a single workspace member.
///
/// Paths inside `autoload` and `autoload_dev` are relative to `dir`. The
/// autoload builder resolves them against that base when generating
/// `vendor/autoload.php` so member source trees are registered without being
/// copied into `vendor/`.
pub struct MemberAutoload {
    /// The member directory, used as the base for all relative autoload paths.
    /// Joined, never canonicalized, so a relative base stays relative all the way
    /// into the generated map.
    pub dir: PathBuf,
    /// Production autoload rules (PSR-4, classmap, files) resolved from the member manifest.
    pub autoload: AutoloadSection,
    /// Dev autoload rules resolved from the member manifest.
    pub autoload_dev: AutoloadSection,
}
