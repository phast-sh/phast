//! Declarative workflows: a real project plus a spec of what Phast should do to it.
//!
//! A workflow is a directory under `testing/e2e/workflows/`. Two names inside it are reserved:
//!
//! - `workflow.toml` is the spec, deserialized into [`Spec`];
//! - `snapshots/` holds the expected output, compared against by a step with `matches`.
//!
//! Everything else is input and is copied verbatim into a scratch directory before the
//! first step runs. Workflow directories are never written to, so a workflow whose command
//! generates its own manifest (`phast init`) keeps that manifest under `snapshots/`
//! rather than beside the spec, where it would read as an input.
//!
//! ```toml
//! description = "A [conflict] entry forces resolution away from the newest major."
//! tier    = "focused"
//! network = true
//!
//! [[step]]
//! args   = ["install", "--dry-run"]
//! stdout = ["psr/log"]
//!
//! [[step]]
//! args    = ["install"]
//! creates = ["phast.lock", "vendor/autoload.php"]
//! ```
//!
//! Steps run in order against one scratch directory, so each sees what the previous
//! left behind. A step asserts exit code 0 unless it says otherwise, which keeps the
//! common case quiet and makes an expected failure explicit.

use crate::hints;
use crate::{phast_cmd, temp_dir};

use std::fs::{copy, create_dir_all, read_dir};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// Spec file name, reserved inside a workflow directory.
pub const SPEC: &str = "workflow.toml";

/// Snapshot directory name, reserved inside a workflow directory.
pub const SNAPSHOTS: &str = "snapshots";

/// Extension marking a snapshot, stripped when resolving what it compares against.
pub const SNAP: &str = "snap";

/// Environment variable that opts in to workflows requiring network access.
pub const NETWORK_ENV: &str = "PHAST_TEST_NETWORK";

/// Environment variable naming the PHP interpreter, read by the binary under test.
pub const PHP_ENV: &str = "PHAST_PHP_BIN";

/// A workflow directory and the spec it holds.
#[derive(Debug)]
pub struct Workflow {
    /// Directory name, used to identify the workflow in failure messages.
    pub name: String,
    /// Path to the workflow directory.
    pub dir: PathBuf,
    /// The parsed spec.
    pub spec: Spec,
}

impl Workflow {
    /// Read and parse the spec in `dir`.
    pub fn load(dir: &Path) -> Result<Self> {
        let name = dir
            .file_name()
            .and_then(|name| name.to_str())
            .with_context(|| format!("workflow directory has no usable name: {}", dir.display()))?
            .to_string();

        let spec = dir.join(SPEC);
        let raw = std::fs::read_to_string(&spec)
            .with_context(|| format!("workflow '{name}' has no {SPEC}"))?;
        let spec: Spec = toml::from_str(&raw)
            .with_context(|| format!("workflow '{name}' has an invalid {SPEC}"))?;

        Ok(Self {
            name,
            dir: dir.to_path_buf(),
            spec,
        })
    }

    /// Why this workflow cannot run here, or `None` when nothing stands in its way.
    ///
    /// Both requirements are declared by the spec rather than discovered mid-run, and each
    /// reports in its own words, because the two are not the same kind of absence. A
    /// registry is opt-in through [`NETWORK_ENV`], so the offline workflows stay usable on
    /// a pull request. A PHP interpreter is detected instead: it is not a choice the runner
    /// makes, and its absence says nothing about Phast.
    pub fn skip_reason(&self) -> Option<String> {
        if self.spec.network && !network_available() {
            return Some(format!("set {NETWORK_ENV} to run it"));
        }

        if self.spec.php && !php_available() {
            return Some(format!(
                "no php interpreter: set {PHP_ENV}, or put php on PATH",
            ));
        }

        None
    }

    /// Copy the inputs to a scratch directory and run every step against it.
    ///
    /// Steps share one cache, as they share one workdir: a workflow is a sequence of
    /// commands in a single project. Neither outlives the run.
    pub fn run(&self) -> Result<()> {
        let workdir = temp_dir();
        let cache = temp_dir();

        copy_inputs(&self.dir, workdir.path())
            .with_context(|| format!("workflow '{}': failed to stage inputs", self.name))?;

        for (index, step) in self.spec.steps.iter().enumerate() {
            self.execute_step(index, step, workdir.path(), cache.path())
                .with_context(|| {
                    format!(
                        "workflow '{}' step {}: `phast {}`",
                        self.name,
                        index + 1,
                        step.args.join(" "),
                    )
                })?;
        }

        Ok(())
    }

    /// Execute one step and check every assertion it declares.
    fn execute_step(
        &self,
        index: usize,
        step: &Step,
        workdir: &Path,
        cache_dir: &Path,
    ) -> Result<()> {
        let output = phast_cmd(workdir, cache_dir)
            .args(&step.args)
            .output()
            .context("failed to invoke the phast binary")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let status = output.status.code();
        if status != Some(step.status) {
            bail!(hints::unexpected_status(
                step.status,
                status,
                &stdout,
                &stderr,
            ));
        }

        for needle in &step.stdout {
            if !stdout.contains(needle.as_str()) {
                bail!(hints::stdout_missing(needle, &stdout));
            }
        }

        for needle in &step.stderr {
            if !stderr.contains(needle.as_str()) {
                bail!(hints::stderr_missing(needle, &stderr));
            }
        }

        for path in &step.creates {
            if !workdir.join(path).exists() {
                bail!(hints::not_created(path));
            }
        }

        for path in &step.absent {
            if workdir.join(path).exists() {
                bail!(hints::not_absent(path));
            }
        }

        if step.matches {
            self.validate_output(workdir)
                .with_context(|| format!("step {} declares matches", index + 1))?;
        }

        Ok(())
    }

    /// Compare every file under the workflow's `snapshots/` tree against the workdir.
    fn validate_output(&self, workdir: &Path) -> Result<()> {
        let snapshots = self.dir.join(SNAPSHOTS);
        if !snapshots.is_dir() {
            bail!(hints::no_snapshots_dir());
        }

        for snapshot in files_under(&snapshots)? {
            let relative = target_path(
                snapshot
                    .strip_prefix(&snapshots)
                    .expect("walked path is under root"),
            );

            let actual = workdir.join(&relative);
            if !actual.exists() {
                bail!(hints::not_produced(&relative));
            }

            let expected = std::fs::read_to_string(&snapshot)
                .with_context(|| format!("failed to read {}", snapshot.display()))?;
            let received = std::fs::read_to_string(&actual)
                .with_context(|| format!("failed to read {}", actual.display()))?;

            if expected != received {
                bail!(hints::snapshot_mismatch(&relative, &expected, &received));
            }
        }

        Ok(())
    }
}

/// What a workflow covers, which decides when it runs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// One behavior, asserted precisely. The default.
    #[default]
    Focused,
    /// A real application, asserted loosely. Slow, and nightly only.
    Ecosystem,
}

/// A parsed `workflow.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    /// What this workflow pins down. Shown when it fails.
    pub description: String,
    /// Which schedule the workflow belongs to.
    #[serde(default)]
    pub tier: Tier,
    /// Whether the workflow reaches a live registry.
    #[serde(default)]
    pub network: bool,
    /// Whether the workflow executes PHP, and so needs an interpreter present.
    #[serde(default)]
    pub php: bool,
    /// Commands to run in order, against one scratch directory.
    #[serde(default, rename = "step")]
    pub steps: Vec<Step>,
}

/// One command invocation and everything asserted about it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Arguments passed to the binary, without the program name.
    pub args: Vec<String>,
    /// Expected exit code.
    #[serde(default)]
    pub status: i32,
    /// Substrings that must appear on stdout.
    #[serde(default)]
    pub stdout: Vec<String>,
    /// Substrings that must appear on stderr.
    #[serde(default)]
    pub stderr: Vec<String>,
    /// Paths, relative to the workdir, that must exist after the step.
    #[serde(default)]
    pub creates: Vec<PathBuf>,
    /// Paths, relative to the workdir, that must not exist after the step.
    #[serde(default)]
    pub absent: Vec<PathBuf>,
    /// Compare every file under `snapshots/` against its workdir counterpart.
    #[serde(default)]
    pub matches: bool,
}

/// Whether workflows needing a live registry have been opted in to, via [`NETWORK_ENV`].
///
/// Presence is the whole signal: the variable is a switch, so any value turns it on.
fn network_available() -> bool {
    std::env::var_os(NETWORK_ENV).is_some()
}

/// Whether a PHP interpreter can be run.
///
/// Resolved the way the binary under test resolves it, so the gate and the `@php` a script
/// eventually runs cannot disagree: [`PHP_ENV`] when set, otherwise `php` on PATH.
fn php_available() -> bool {
    let php = std::env::var(PHP_ENV).unwrap_or_else(|_| "php".to_owned());

    std::process::Command::new(php)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Map a path inside `snapshots/` to the workdir path it is compared against.
///
/// A trailing [`SNAP`] extension is stripped, so `phast.toml.snap` is compared
/// against `phast.toml`. Snapshots are asserted byte for byte, which makes them
/// exactly the files an editor or formatter must not touch, and an extension no
/// formatter recognizes is the only reliable way to keep them out of reach.
#[rustfmt::skip]
fn target_path(relative: &Path) -> PathBuf {
    match relative.extension() {
        Some(ext) if ext == SNAP => relative.with_extension(""),
                               _ => relative.to_path_buf(),
    }
}

/// Copy a workflow directory into `dest`, skipping the two reserved names.
fn copy_inputs(workdir: &Path, dest: &Path) -> Result<()> {
    for file in read_dir(workdir)? {
        let path = file?.path();
        let name = path.file_name().expect("directory entry has a name");

        if name == SPEC || name == SNAPSHOTS {
            continue;
        }

        let target = dest.join(name);
        if path.is_dir() {
            copy_tree(&path, &target)?;
        } else {
            copy(&path, &target)?;
        }
    }

    Ok(())
}

/// Recursively copy `from` to `to`, creating `to` and any parents.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    create_dir_all(to)?;

    for file in read_dir(from)? {
        let path = file?.path();
        let target = to.join(path.file_name().expect("directory entry has a name"));

        if path.is_dir() {
            copy_tree(&path, &target)?;
        } else {
            copy(&path, &target)?;
        }
    }

    Ok(())
}

/// Every file under `root`, recursively, in no particular order.
fn files_under(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for file in read_dir(root)? {
        let path = file?.path();
        if path.is_dir() {
            files.extend(files_under(&path)?);
        } else {
            files.push(path);
        }
    }

    Ok(files)
}
