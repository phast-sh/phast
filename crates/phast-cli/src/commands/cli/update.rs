//! `phast cli update`: replace the running binary with the latest GitHub release.
//!
//! The flow mirrors `install.sh`, in Rust, split across four types:
//!
//! - [`SelfUpdater`] orchestrates: compare versions, then download, verify,
//!   extract, and install.
//! - `ReleaseSource` is the remote: it talks to the GitHub release API and
//!   downloads assets. The only type that assumes GitHub.
//! - [`Archive`] is the downloaded `phast-<target>.tar.gz`: it verifies its own
//!   checksum and extracts the `phast` binary.
//! - `Installer` swaps a new binary in over the running executable with an
//!   atomic rename. Anything that fails before that rename leaves the installed
//!   binary untouched.
//!
//! The asset naming and target triple come from the release workflow and
//! `build.rs` (`PHAST_TARGET`); the repository slug from [`phast_core::meta`].

use crate::output::Printer;
use crate::release;

use std::env::current_exe;
use std::ffi::OsStr;
use std::fs::canonicalize;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::str::from_utf8;

use phast_core::meta;
use phast_version::parser::parse_version;

use anyhow::{Context, Error, Result, anyhow, bail};
use flate2::read::GzDecoder;
use reqwest::Client;
use sha2::{Digest, Sha256};
use tar::Archive as TarArchive;
use tempfile::Builder;

/// The binary's name, both the asset's inner file and the installed file.
const BIN: &str = "phast";

/// The target triple this build was compiled for, baked in by `build.rs`.
const TARGET: &str = env!("PHAST_TARGET");

/// Drives `phast cli update`: the end-to-end orchestration.
pub struct SelfUpdater<'a> {
    printer: &'a Printer,
}

#[rustfmt::skip]
impl<'a> SelfUpdater<'a> {
    pub fn new(printer: &'a Printer) -> Self {
        Self { printer }
    }

    /// Check for a newer release and, if there is one, download and install it.
    pub async fn update(&self) -> Result<()> {
        let target = known_target()?;
        let source = ReleaseSource::new()?;

        let spinner = self.printer.spinner("Checking for the latest release...");

        let tag = source
            .latest_tag()
            .await?;
        let latest = parse_version(&tag)
            .with_context(|| format!("could not parse the latest release version '{tag}'"))?;
        let current = parse_version(release::RELEASE_VERSION)
            .context("could not parse the running version")?;

        if current >= latest {
            spinner.finish_and_clear();
            self.printer.success(
                format!("phast {} is already up to date", release::RELEASE_VERSION),
            );
            return Ok(());
        }

        spinner.println(format!("Updating phast {current} -> {latest}"));

        let asset = format!("{BIN}-{target}.tar.gz");
        let bytes = source
            .download(&tag, &asset)
            .await?;
        let checksum = source
            .download(&tag, &format!("{asset}.sha256"))
            .await?;

        let archive  = Archive::new(bytes);
        archive.verify(&checksum)?;

        let binary = archive
            .extract()?;
        Installer::for_current_exe()?
            .install(&binary)?;

        spinner.finish_and_clear();
        self.printer.success(format!("Updated phast to {latest}"));
        Ok(())
    }
}

/// The remote release source: the GitHub release API and asset downloads.
///
/// The single type that assumes GitHub; everything host-specific (the API host,
/// the asset URL layout, the [`slug`]) lives here.
struct ReleaseSource {
    client: Client,
}

impl ReleaseSource {
    /// Build a client carrying the shared phast user-agent (the GitHub API requires one).
    fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent(meta::user_agent())
            .build()
            .context("failed to build the HTTP client")?;
        Ok(Self { client })
    }

    /// The `tag_name` of the latest (non-prerelease, non-draft) release.
    async fn latest_tag(&self) -> Result<String> {
        /// The single field we read from the GitHub release API response.
        #[derive(serde::Deserialize)]
        struct LatestRelease {
            tag_name: String,
        }

        let url = format!("https://api.github.com/repos/{}/releases/latest", slug());

        let response: reqwest::Response = self
            .client
            .get(&url)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .context("failed to query the latest release")?
            .error_for_status()
            .context("the GitHub release API returned an error")?;

        let data: LatestRelease = response
            .json()
            .await
            .context("could not parse the release API response")?;

        Ok(data.tag_name)
    }

    /// Download a release asset's bytes, failing on a non-success status.
    async fn download(&self, tag: &str, asset: &str) -> Result<Vec<u8>> {
        let url = format!(
            "https://github.com/{}/releases/download/{tag}/{asset}",
            slug()
        );

        let response: reqwest::Response = self
            .client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("failed to request {url}"))?;

        let bytes = response
            .error_for_status()
            .with_context(|| format!("HTTP error fetching {url}"))?
            .bytes()
            .await
            .with_context(|| format!("failed to read the response body from {url}"))?;

        Ok(bytes.to_vec())
    }
}

/// A downloaded release archive (`phast-<target>.tar.gz`): it verifies its own
/// checksum and extracts the `phast` binary.
pub struct Archive {
    /// The raw `.tar.gz` bytes, verified before anything is extracted from them.
    bytes: Vec<u8>,
}

impl Archive {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Fail unless the archive hashes to the sha256 recorded in `checksum`
    /// (`<hex>  <name>`, as `sha256sum` writes it).
    pub fn verify(&self, checksum: &[u8]) -> Result<()> {
        let checksum = from_utf8(checksum).context("checksum file is not valid UTF-8")?;
        let expected = checksum
            .split_whitespace()
            .next()
            .context("checksum file is empty")?;

        let actual = hex::encode(Sha256::digest(&self.bytes));
        if !actual.eq_ignore_ascii_case(expected) {
            bail!("checksum mismatch: expected {expected}, got {actual}");
        }

        Ok(())
    }

    /// Pull the `phast` binary out of the gzipped tarball.
    ///
    /// The release archive holds the binary at its root, so this looks for an
    /// entry whose file name is `phast` rather than walking a tree.
    pub fn extract(&self) -> Result<Vec<u8>> {
        let decoder = GzDecoder::new(self.bytes.as_slice());
        let mut tar = TarArchive::new(decoder);

        for entry in tar
            .entries()
            .context("failed to read the release archive")?
        {
            let mut entry = entry.context("corrupt entry in the release archive")?;
            let is_binary = entry
                .path()
                .ok()
                .and_then(|p| p.file_name().map(|n| n == OsStr::new(BIN)))
                .unwrap_or(false);
            if !is_binary {
                continue;
            }

            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .context("failed to read the phast binary from the archive")?;

            return Ok(buf);
        }

        bail!("the release archive did not contain a '{BIN}' binary");
    }
}

/// The install target: the running executable, replaced atomically.
struct Installer {
    exe: PathBuf,
}

#[rustfmt::skip]
impl Installer {
    /// Locate the running executable (following symlinks) as the install target.
    fn for_current_exe() -> Result<Self> {
        let exe = current_exe()
            .context("could not locate the running phast executable")?;
        let exe = canonicalize(&exe)
            .unwrap_or(exe);
        Ok(Self { exe })
    }

    /// Write `binary` next to the executable and atomically rename it into place.
    ///
    /// The temp file shares the executable's directory so the rename stays on one
    /// filesystem (and is therefore atomic); replacing a running binary is safe on
    /// Unix, where the live process keeps the old inode.
    fn install(&self, binary: &[u8]) -> Result<()> {
        let dir = self
            .exe
            .parent()
            .with_context(|| format!("{} has no parent directory", self.exe.display()))?;

        let mut tmp = Builder::new()
            .prefix(".phast-update-")
            .tempfile_in(dir)
            .map_err(|e| Self::io_error(e, dir, "failed to stage the update"))?;

        tmp.write_all(binary)
            .context("failed to write the new binary")?;
        tmp.as_file()
            .sync_all()
            .context("failed to flush the new binary to disk")?;

        let mut perms = tmp
            .as_file()
            .metadata()?
            .permissions();
        perms.set_mode(0o755);

        tmp.as_file()
            .set_permissions(perms)
            .context("failed to mark the new binary executable")?;

        tmp.persist(&self.exe)
            .map_err(|e| Self::io_error(e.error, &self.exe, "failed to replace the binary"))?;
        Ok(())
    }

    /// Map a filesystem error to a permission-aware message, pointing at a
    /// privileged reinstall when the target is not writable.
    fn io_error(err: std::io::Error, path: &Path, what: &str) -> Error {
        if err.kind() != ErrorKind::PermissionDenied {
            return Error::new(err)
                .context(format!("{what} at {}", path.display()));
        }

        anyhow!(
            "no permission to write {}.\n\
             Re-run with sufficient permissions, or reinstall with:\n  {}",
            path.display(),
            reinstall_hint(),
        )
    }
}

/// This build's target triple, or an error pointing at the installer for a build
/// that has no embedded triple (one made outside the release pipeline).
fn known_target() -> Result<&'static str> {
    match TARGET {
        "unknown" => bail!(
            "this build has no embedded target triple, so it cannot select a release asset.\n\
             Reinstall with:\n  {}",
            reinstall_hint(),
        ),
        target => Ok(target),
    }
}

/// The one-line installer command, derived from the repository slug.
fn reinstall_hint() -> String {
    format!(
        "curl -fsSL https://raw.githubusercontent.com/{}/main/install.sh | sh",
        slug(),
    )
}

/// The `<owner>/<repo>` slug for the GitHub API and asset URLs.
///
/// Derived from the project's repository URL by stripping the GitHub host.
fn slug() -> &'static str {
    meta::repository_url()
        .strip_prefix("https://github.com/")
        .expect(
            "phast cli update speaks the GitHub release API; \
             [package].repository must be a github.com URL",
        )
}
