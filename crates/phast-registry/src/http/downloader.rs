//! Package archive downloading and extraction.
//!
//! Downloads zip archives from `dist.url` into the archive cache, then
//! extracts them into `vendor/{vendor}/{package}/`.

use crate::archive::{extractor, integrity};
use crate::hints;
use crate::http::cache;
use crate::http::cache::Cache;
use crate::http::client::HttpClient;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use phast_core::lock::phast::{DistType, PackageDef};

use anyhow::Context;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinSet;

/// CPU parallelism assumed when the platform will not report its own.
const DEFAULT_CPU_PARALLELISM: usize = 4;

/// Maximum number of concurrent HTTP download tasks, and the capacity of the
/// channel feeding the extraction stage, so a finished download always has
/// somewhere to hand its archive off to.
const NET_CONCURRENCY: usize = 50;

/// Attempts per archive download (one initial try plus retries) before failing.
///
/// GitHub's `codeload` endpoint returns a transient `400` while it generates a
/// zipball for a fresh ref, and the archive is ready a few seconds later. The
/// doubling backoff below has to outlast that, so the retry window is measured
/// in seconds rather than in blips.
const DOWNLOAD_ATTEMPTS: u32 = 5;

/// Base delay between download retries, doubled on each subsequent attempt.
const RETRY_DELAY: Duration = Duration::from_millis(300);

/// Service that coordinates fetching and extracting package archives.
///
/// Holds an HTTP client (with credentials baked in) and a disk cache. Construct
/// once per command invocation and call [`download`](Downloader::download) for
/// each batch of packages.
pub struct Downloader {
    /// HTTP client with credentials baked in.
    client: HttpClient,
    /// Disk cache for downloaded archives (content-addressed by shasum or ref).
    cache: Cache,
    /// Limits concurrent HTTP download tasks.
    net_sem: Arc<Semaphore>,
    /// Limits concurrent `spawn_blocking` extraction tasks to available CPU parallelism.
    cpu_sem: Arc<Semaphore>,
}

impl Downloader {
    /// A downloader bounded for this machine: extraction runs at most one task
    /// per CPU, falling back to a fixed width when the platform will not report
    /// its parallelism.
    pub fn new(client: HttpClient, cache: Cache) -> Self {
        let cpu_parallelism = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(DEFAULT_CPU_PARALLELISM);
        Self {
            client,
            cache,
            net_sem: Arc::new(Semaphore::new(NET_CONCURRENCY)),
            cpu_sem: Arc::new(Semaphore::new(cpu_parallelism)),
        }
    }

    /// Download and extract `packages` into `vendor_dir`.
    ///
    /// Runs as a two-stage pipeline: network tasks fetch archives concurrently
    /// and send completed paths over a channel; extraction tasks pick them up
    /// and run `extract_zip` in a blocking thread pool. The two stages overlap
    /// so CPU and I/O work simultaneously.
    ///
    /// `on_progress` is called with the package name and version after each
    /// package is extracted, in completion order (fastest packages first).
    ///
    /// A package carrying no `dist`, or a `dist` that is not a zip, is skipped
    /// without a callback, so a workspace member or a path package passes through
    /// untouched. The first failure returns while other tasks are still in flight, so
    /// which of the remaining packages reached `vendor/` is not something a caller can
    /// infer from the error.
    pub async fn download(
        &self,
        packages: &[PackageDef],
        vendor_dir: &Path,
        mut on_progress: impl FnMut(&str, &str),
    ) -> anyhow::Result<()> {
        let (tx, mut rx) = mpsc::channel::<PackageTask>(NET_CONCURRENCY);
        let mut download_set: JoinSet<anyhow::Result<()>> = JoinSet::new();
        let mut extract_set: JoinSet<anyhow::Result<(String, String)>> = JoinSet::new();

        for package in packages {
            // Skip packages with nothing to fetch
            // (e.g. workspace members).
            let Some(dist) = &package.dist else {
                continue;
            };
            // This stage only handles zip archives;
            // path dists are materialized outside registry.
            if dist.dist_type != DistType::Zip {
                continue;
            }
            let dist = dist.clone();

            let cache_key = cache::dist_key(&dist).with_context(|| {
                format!("no dist URL for {} to derive a cache key", package.name)
            })?;

            let task = PackageTask {
                url: dist.download_url(&package.version),
                name: package.name.clone(),
                version: package.version.clone(),
                archive_path: self.cache.archive_path(&cache_key),
                package_path: vendor_dir.join(&package.name),
                shasum: dist.shasum.clone(),
            };
            self.spawn_download(&mut download_set, task, tx.clone());
        }

        // Drop the sender owned by this scope so `rx` sees EOF when all
        // download tasks have also dropped their clones.
        drop(tx);

        // Drive both stages concurrently.
        //
        // Three branches run in the same select! loop so that progress is
        // reported as each extraction finishes, interleaved with ongoing
        // downloads rather than only after all downloads are done:
        //
        //   1. Collect download errors as they occur.
        //   2. Spawn an extraction task for each freshly-fetched archive.
        //   3. Call `on_progress` as each extraction completes.
        //
        // The `else` branch fires only when all three JoinSets/channels are
        // exhausted (patterns return `None`), i.e. everything is done.
        loop {
            tokio::select! {
                Some(result) = download_set.join_next() => {
                    result.context("download task panicked")??;
                }
                Some(task) = rx.recv() => {
                    self.spawn_extract(&mut extract_set, task);
                }
                Some(result) = extract_set.join_next() => {
                    let (name, version) = result.context("extract task panicked")??;
                    on_progress(&name, &version);
                }
                else => break,
            }
        }

        Ok(())
    }

    /// Spawn a network-bounded task that fetches the archive and forwards the task to the extraction stage.
    fn spawn_download(
        &self,
        set: &mut JoinSet<anyhow::Result<()>>,
        task: PackageTask,
        tx: mpsc::Sender<PackageTask>,
    ) {
        let client = self.client.clone();
        let net_sem = Arc::clone(&self.net_sem);

        set.spawn(async move {
            let _permit = net_sem.acquire_owned().await;
            Self::fetch_archive(&client, &task).await?;
            tx.send(task)
                .await
                .map_err(|_| anyhow::anyhow!("pipeline channel closed unexpectedly"))?;
            Ok(())
        });
    }

    /// Spawn a CPU-bounded blocking task that extracts the archive into the package directory.
    fn spawn_extract(
        &self,
        set: &mut JoinSet<anyhow::Result<(String, String)>>,
        task: PackageTask,
    ) {
        let cpu_sem = Arc::clone(&self.cpu_sem);

        set.spawn(async move {
            let _permit = cpu_sem.acquire_owned().await;
            tokio::task::spawn_blocking(move || {
                // Read the archive once, then both verify and extract from the
                // same in-memory bytes. This is the single integrity checkpoint:
                // it covers freshly downloaded archives and cache hits alike,
                // so a corrupt or tampered cache entry can never reach `vendor/`.
                let bytes = std::fs::read(&task.archive_path).with_context(|| {
                    format!("failed to read archive {}", task.archive_path.display())
                })?;

                // Verify integrity before extracting. Hashing and comparison live in
                // `integrity::verify_sha1`; the downloader owns the recovery policy:
                // a missing `shasum` passes through unverified, and on a mismatch
                // the poisoned cache entry is deleted so the next run re-downloads
                // a clean copy instead of failing forever.
                verify_checksum(&task, &bytes)?;

                extractor::extract_zip(&bytes, &task.package_path).with_context(|| {
                    format!("failed to extract {}", task.archive_path.display())
                })?;

                Ok((task.name, task.version))
            })
            .await
            .context("extractor task panicked")?
        });
    }

    /// Fetch a zip archive from `task.url` and write it to `task.archive_path`.
    ///
    /// Returns immediately if `task.archive_path` already exists (cache hit).
    async fn fetch_archive(client: &HttpClient, task: &PackageTask) -> anyhow::Result<()> {
        if task.archive_path.exists() {
            return Ok(());
        }

        let bytes = Self::download_bytes(client, &task.url).await?;

        // Integrity is verified later, in the extraction stage, so it covers cache
        // hits too. See `spawn_extract` / `verify_checksum`.

        if let Some(parent) = task.archive_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create archive cache dir {}", parent.display())
            })?;
        }

        std::fs::write(&task.archive_path, &bytes).with_context(|| {
            format!("failed to write archive to {}", task.archive_path.display())
        })?;

        Ok(())
    }

    /// GET `url` and return the body, retrying transient failures with backoff.
    async fn download_bytes(client: &HttpClient, url: &str) -> anyhow::Result<bytes::Bytes> {
        let mut attempt: u32 = 1;
        loop {
            let error = match Self::try_download(client, url).await {
                Ok(bytes) => return Ok(bytes),
                Err(e) => e,
            };

            if attempt >= DOWNLOAD_ATTEMPTS {
                return Err(error);
            }

            tokio::time::sleep(RETRY_DELAY * 2u32.pow(attempt - 1)).await;
            attempt += 1;
        }
    }

    /// One download attempt: GET `url`, fail on a non-success status, read the body.
    async fn try_download(client: &HttpClient, url: &str) -> anyhow::Result<bytes::Bytes> {
        let response: reqwest::Response = client
            .get(url)?
            .send()
            .await
            .with_context(|| format!("failed to fetch {url}"))?
            .error_for_status()
            .with_context(|| format!("HTTP error fetching {url}"))?;

        let bytes: bytes::Bytes = response
            .bytes()
            .await
            .with_context(|| format!("failed to read response body from {}", url))?;

        Ok(bytes)
    }
}

/// All paths and identifiers needed to download and extract one package.
struct PackageTask {
    /// Download URL for the dist archive.
    url: String,
    /// Vendor-scoped package name, e.g. `vendor/package`.
    name: String,
    /// Resolved version string, e.g. `1.2.3`.
    version: String,
    /// Path to the cached zip archive on disk.
    archive_path: PathBuf,
    /// Destination directory inside `vendor/` where the archive is extracted.
    package_path: PathBuf,
    /// Expected SHA-1 checksum of the archive, as published by Packagist.
    /// Verified in the extraction stage.
    shasum: Option<String>,
}

/// Check `bytes` against the package's expected SHA-1 and apply the cache
/// recovery policy on failure.
#[rustfmt::skip]
fn verify_checksum(task: &PackageTask, bytes: &[u8]) -> anyhow::Result<()> {
    let expected = task
        .shasum
        .as_deref()
        .unwrap_or_default();

    let Err(actual) = integrity::verify_sha1(bytes, expected) else {
        return Ok(());
    };

    // Mismatch: drop the poisoned cache entry so the next run re-downloads it.
    let _ = std::fs::remove_file(&task.archive_path);
    anyhow::bail!(hints::checksum_mismatch(
        &task.name,
        &task.version,
        expected,
        &actual,
    ));
}
