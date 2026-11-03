//! Safe zip extraction for downloaded package archives.

use std::fs::File;
use std::io::Cursor;
use std::path::{Component, Path};

use anyhow::{Context, Result};
use zip::ZipArchive;

/// Extract a zip archive from in-memory `bytes` into `dest_dir`, stripping the
/// top-level directory.
///
/// Takes bytes rather than a path so the archive is read once, with the same
/// bytes serving both the checksum check and the extraction.
///
/// A Packagist archive wraps its contents in a single root directory named
/// `{vendor}-{package}-{ref}/`, which is stripped so they land directly at
/// `dest_dir/`. An archive whose entries share no such root is extracted as it
/// stands.
///
/// Entry permissions are not carried over, so a file stored `0755` is written
/// `0644`. An entry whose path contains `..` or a leading `/` fails the whole
/// extraction, with whatever was written before it left behind.
#[rustfmt::skip]
pub fn extract_zip(bytes: &[u8], dest_dir: &Path) -> Result<()> {
    let mut zip = ZipArchive::new(Cursor::new(bytes))
        .context("failed to open zip archive")?;

    let prefix = find_zip_prefix(&mut zip);

    std::fs::create_dir_all(dest_dir)
        .with_context(|| format!("failed to create {}", dest_dir.display()))?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let arch_name = entry
            .name()
            .context("archive entry has a name that cannot be read")?
            .into_owned();

        let relative = match prefix.as_deref() {
            Some(p) => arch_name.strip_prefix(p).unwrap_or(&arch_name),
            None    => &arch_name,
        };
        if relative.is_empty() {
            continue;
        }

        // Guard against zip-slip: every component must be a plain name.
        // Rejecting ParentDir ("..") and RootDir ("/") before joining means
        // dest is always strictly inside dest_dir, regardless of how the OS
        // resolves the final path.
        let entry_path = Path::new(relative);
        let is_unsafe = entry_path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)));
        if is_unsafe {
            anyhow::bail!(
                "archive entry '{}' contains a non-normal path component",
                arch_name,
            );
        }

        let dest = dest_dir.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest)?;
            continue;
        }

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = File::create(&dest)
            .with_context(|| format!("failed to create {}", dest.display()))?;
        std::io::copy(&mut entry, &mut file)?;
    }

    Ok(())
}

/// Find the common top-level directory prefix in a zip archive, if any.
///
/// The candidate is the first entry's leading path segment, returned only if every
/// other entry starts with it. Nothing checks that the segment names a directory, so
/// an archive holding the single file `a.php` yields `Some("a.php/")`, a prefix that
/// matches no entry and so strips nothing.
fn find_zip_prefix(zip: &mut ZipArchive<Cursor<&[u8]>>) -> Option<String> {
    if zip.is_empty() {
        return None;
    }

    let first = zip.by_index(0).ok()?;
    let name = first.name().ok()?;
    let prefix = name.split('/').next().map(|s| format!("{}/", s))?;
    drop(first);

    for i in 1..zip.len() {
        if let Ok(entry) = zip.by_index(i)
            && !entry.name().is_ok_and(|name| name.starts_with(&prefix))
        {
            return None;
        }
    }

    Some(prefix)
}
