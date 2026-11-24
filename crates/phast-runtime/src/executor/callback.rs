//! Running a PHP static callback (`Vendor\Class::method`).

use super::{Environment, ScriptOutput};
use crate::context::{CallbackContext, Invocation, Target};
use crate::hints;

use std::time::SystemTime;

use anyhow::{Context, Result, bail};

/// The PHP entry point that runs a script callback.
const BOOTSTRAP: &str = include_str!("../stubs/callback/bootstrap.php");
/// Phast's own classes for scripts.
const PHAST: &str = include_str!("../stubs/callback/Phast.php");
/// Composer's classes, as adapters over Phast's.
const COMPOSER: &str = include_str!("../stubs/callback/Composer.php");

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

    /// Call the static method `callback` (e.g. `Acme\Hooks\Report::show`) with the current
    /// project and event.
    #[rustfmt::skip]
    pub(super) fn call(&self, callback: &str, output: &impl ScriptOutput) -> Result<()> {
        output.info(&format!("> {callback}"));

        let (class, method) = callback
            .split_once("::")
            .context("invalid PHP callback: expected 'Vendor\\Class::method'")?;

        let callback_dir = std::env::temp_dir().join(format!(
            "phast_cb_{}",
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir(&callback_dir)
            .context("failed to create PHP callback directory")?;
        std::fs::write(callback_dir.join("bootstrap.php"), BOOTSTRAP)
            .context("failed to write PHP callback script")?;
        std::fs::write(callback_dir.join("Phast.php"), PHAST)
            .context("failed to write PHP callback script")?;
        std::fs::write(callback_dir.join("Composer.php"), COMPOSER)
            .context("failed to write PHP callback script")?;

        let context_path = callback_dir.join("context.json");
        let context_body = CallbackContext {
            callback:   Target { class, method },
            project:    self.environment.project.as_ref(),
            invocation: self.invocation,
        };
        std::fs::write(&context_path, serde_json::to_string(&context_body)?)
            .context("failed to write PHP callback context")?;

        let bootstrap_path = callback_dir.join("bootstrap.php");
        let status = self.environment.command(&self.environment.php)
            .arg(bootstrap_path)
            .arg(&context_path)
            .status()
            .context(hints::shell_start_failed())?;

        let _ = std::fs::remove_dir_all(&callback_dir);

        if !status.success() {
            bail!(hints::script_failed(status));
        }
        Ok(())
    }
}
