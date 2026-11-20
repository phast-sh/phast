//! Running a PHP static callback (`Vendor\Class::method`).

use super::{Environment, ScriptOutput};
use crate::context::Invocation;
use crate::hints;

use std::time::SystemTime;

use anyhow::{Context, Result, bail};

/// One PHP callback, as part of one run.
pub(super) struct Callback<'a> {
    environment: &'a Environment,
    invocation: &'a Invocation,
}

impl<'a> Callback<'a> {
    /// A callback run in `environment`, as part of the run that is described by `invocation`.
    pub(super) fn new(environment: &'a Environment, invocation: &'a Invocation) -> Self {
        Self {
            environment,
            invocation,
        }
    }

    /// Call `callback` by writing a temporary PHP bootstrap (from `stubs/callback_bootstrap.php`)
    /// that requires the project autoloader, provides a minimal `Composer\Script\Event` stub when
    /// the real one is absent, and invokes the method via reflection so handlers that accept
    /// no arguments also work without a type error.
    ///
    /// The temporary file is removed whether or not the callback succeeded, and
    /// its name carries a nanosecond timestamp so concurrent runs cannot collide.
    #[rustfmt::skip]
    pub(super) fn call(&self, callback: &str, output: &impl ScriptOutput) -> Result<()> {
        output.info(&format!("> {callback}"));

        let (class, method) = callback
            .split_once("::")
            .context("invalid PHP callback: expected 'Vendor\\Class::method'")?;

        let php = include_str!("../stubs/callback_bootstrap.php")
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

        let status = self.environment.command(&self.environment.php)
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
