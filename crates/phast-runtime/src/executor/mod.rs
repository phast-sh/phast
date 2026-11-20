//! Running script entries.
//!
//! An [`Environment`] holds what stays the same for a whole command: the PHP binary, the
//! working directory and the project. Each script runs through an [`Executor`], which adds
//! that run's invocation.

mod callback;

use callback::Callback;

use crate::context::{Invocation, Project};
use crate::hints;
use crate::php;
use crate::runner::ScriptEntry;

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// What every script in one command runs with: the PHP binary, the working directory and
/// the project.
///
/// Built once per command and shared by all of its hooks. Each script then runs through
/// an [`Executor`].
#[derive(Debug)]
pub struct Environment {
    /// The PHP interpreter to invoke, resolved once at construction so a
    /// sequence of entries cannot disagree about which PHP they ran under.
    php: String,
    /// Directory to run commands in. `None` means the process working
    /// directory, which is wherever Phast itself was invoked, not necessarily
    /// the project being acted on.
    dir: Option<PathBuf>,
    /// The project a PHP callback is told about. `None` means callbacks get
    /// no project data.
    project: Option<Project>,
}

impl Environment {
    /// Create an `Environment`, resolving the PHP binary via [`php::binary`].
    ///
    /// Commands run in the process working directory until [`with_dir`](Self::with_dir)
    /// says otherwise.
    pub fn new() -> Self {
        Self {
            php: php::binary(),
            dir: None,
            project: None,
        }
    }

    /// Run all commands in `dir` instead of the process working directory.
    pub fn with_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dir = Some(dir.into());
        self
    }

    /// Tell every PHP callback about `project`.
    pub fn with_project(mut self, project: Project) -> Self {
        self.project = Some(project);
        self
    }

    /// Create a `Command`, applying the configured working directory if any.
    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        if let Some(dir) = &self.dir {
            command.current_dir(dir);
        }
        command
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs the entries of one script, in an environment.
pub struct Executor<'a> {
    environment: &'a Environment,
    invocation: &'a Invocation,
}

impl<'a> Executor<'a> {
    /// An executor for the run `invocation` describes, in `environment`.
    pub fn new(environment: &'a Environment, invocation: &'a Invocation) -> Self {
        Self {
            environment,
            invocation,
        }
    }

    /// Run one entry of the script.
    #[rustfmt::skip]
    pub fn execute(&self, entry: &ScriptEntry, output: &impl ScriptOutput) -> Result<()> {
        match entry {
            ScriptEntry::Shell(cmd)       => self.execute_shell(cmd, output),
            ScriptEntry::PhpCommand(args) => self.execute_php_command(args, output),
            ScriptEntry::PhpCallback(cb)  => Callback::new(self.environment, self.invocation).call(cb, output),
            ScriptEntry::Reference(name)  => bail!(hints::reference_not_expanded(name)),
        }
    }

    /// Run a bare shell command via `sh -c`.
    fn execute_shell(&self, cmd: &str, output: &impl ScriptOutput) -> Result<()> {
        output.info(&format!("> {cmd}"));
        let status = self
            .environment
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
            .environment
            .command("sh")
            .arg("-c")
            .arg(format!("{} {args}", self.environment.php))
            .status()
            .context(hints::shell_start_failed())?;
        if !status.success() {
            bail!(hints::script_failed(status));
        }
        Ok(())
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
