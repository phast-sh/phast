//! Script entry execution.
//!
//! `ScriptOutput` is a thin trait so callers can inject their own printer
//! without creating a dependency on any specific output implementation.
//! `Executor` owns the resolved PHP binary path and runs whichever kind of
//! entry it is handed. Printing `> {cmd}` happens here, before each command
//! is run, so an entry echoes itself however it was classified.

use crate::hints;
use crate::php;
use crate::runner::ScriptEntry;

use std::path::PathBuf;
use std::process::Command;
use std::time::SystemTime;

use anyhow::{Context, Result, bail};

/// Runs a [`ScriptEntry`] against the OS, whichever kind it turned out to be.
///
/// Owns the resolved PHP binary path. Constructed once per script invocation
/// and reused across all entries in the sequence.
#[derive(Debug)]
pub struct Executor {
    /// The PHP interpreter to invoke, resolved once at construction so a
    /// sequence of entries cannot disagree about which PHP they ran under.
    php_bin: String,
    /// Directory to run commands in. `None` means the process working
    /// directory, which is wherever Phast itself was invoked, not necessarily
    /// the project being acted on.
    dir: Option<PathBuf>,
}

impl Executor {
    /// Create an `Executor`, resolving the PHP binary via [`php::binary`].
    /// Commands run in the process working directory until [`with_dir`](Self::with_dir)
    /// says otherwise.
    pub fn new() -> Self {
        Self {
            php_bin: php::binary(),
            dir: None,
        }
    }

    /// Run all commands in `dir` instead of the process working directory.
    pub fn with_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dir = Some(dir.into());
        self
    }

    /// Dispatch a single `ScriptEntry` to the appropriate execute method.
    #[rustfmt::skip]
    pub fn execute(&self, entry: &ScriptEntry, output: &impl ScriptOutput) -> Result<()> {
        match entry {
            ScriptEntry::Shell(cmd)       => self.execute_shell(cmd, output),
            ScriptEntry::PhpCommand(args) => self.execute_php_command(args, output),
            ScriptEntry::PhpCallback(cb)  => self.execute_php_callback(cb, output),
        }
    }

    /// Create a `Command`, applying the configured working directory if any.
    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        if let Some(dir) = &self.dir {
            command.current_dir(dir);
        }
        command
    }

    /// Run a bare shell command via `sh -c`.
    fn execute_shell(&self, cmd: &str, output: &impl ScriptOutput) -> Result<()> {
        output.info(&format!("> {cmd}"));
        let status = self
            .command("sh")
            .arg("-c")
            .arg(cmd)
            .status()
            .context(hints::shell_start_failed())?;
        if !status.success() {
            bail!(hints::script_failed(status));
        }
        Ok(())
    }

    /// Run a `@php` command by prepending the resolved PHP binary and passing
    /// the remainder to `sh -c` so shell quoting in `args` is handled correctly.
    fn execute_php_command(&self, args: &str, output: &impl ScriptOutput) -> Result<()> {
        output.info(&format!("> @php {args}"));
        let status = self
            .command("sh")
            .arg("-c")
            .arg(format!("{} {args}", self.php_bin))
            .status()
            .context(hints::shell_start_failed())?;
        if !status.success() {
            bail!(hints::script_failed(status));
        }
        Ok(())
    }

    /// Execute a PHP static callback (`Vendor\Class::method`) by writing a
    /// temporary PHP bootstrap (from `stubs/callback_bootstrap.php`) that
    /// requires the project autoloader, provides a minimal `Composer\Script\Event`
    /// stub when the real one is absent, and invokes the method via reflection so
    /// handlers that accept no arguments also work without a type error.
    ///
    /// The temporary file is removed whether or not the callback succeeded, and
    /// its name carries a nanosecond timestamp so concurrent runs cannot collide.
    #[rustfmt::skip]
    fn execute_php_callback(&self, callback: &str, output: &impl ScriptOutput) -> Result<()> {
        output.info(&format!("> {callback}"));

        let (class, method) = callback
            .split_once("::")
            .context("invalid PHP callback: expected 'Vendor\\Class::method'")?;

        let php = include_str!("stubs/callback_bootstrap.php")
            .replace("{class}", class)
            .replace("{method}", method);

        let tmp = std::env::temp_dir().join(format!(
            "phast_cb_{}.php",
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::write(&tmp, php)
            .context("failed to write PHP callback script")?;

        let status = self.command(&self.php_bin)
            .arg(&tmp)
            .status()
            .context(hints::shell_start_failed())?;

        let _ = std::fs::remove_file(&tmp);

        if !status.success() {
            bail!(hints::script_failed(status));
        }
        Ok(())
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

/// Sink for `> {cmd}` output printed before each script entry is executed.
///
/// A trait rather than a concrete printer so this crate owes nothing to any
/// particular output style, which is also what lets a test capture the lines.
pub trait ScriptOutput {
    /// Emit one informational line.
    fn info(&self, msg: &str);
}
