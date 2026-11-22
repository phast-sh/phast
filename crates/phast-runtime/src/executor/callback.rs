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
    /// The project and this run go to PHP as a JSON file next to the bootstrap. Both files
    /// are removed whether or not the callback succeeded, and their name carries a
    /// nanosecond timestamp so concurrent runs cannot collide.
    ///
    /// For `phast run dump` in `/srv/probe`, the file looks like this. `project` is `null`
    /// when the environment has none, and `extra` and `version` are `null` when the
    /// manifest doesn't declare them.
    ///
    /// ```json
    /// {
    ///   "invocation": {
    ///     "event": "dump",
    ///     "arguments": [],
    ///     "rules": {
    ///       "psr4": { "Acme\\Probe\\": ["/srv/probe/src/"] },
    ///       "psr0": {}
    ///     }
    ///   },
    ///   "project": {
    ///     "name": "acme/probe",
    ///     "version": null,
    ///     "extra": null,
    ///     "require": {},
    ///     "require_dev": {},
    ///     "autoload": {
    ///       "psr-4": { "Acme\\Probe\\": ["src/"] },
    ///       "psr-0": {},
    ///       "classmap": [],
    ///       "files": [],
    ///       "exclude-from-classmap": []
    ///     },
    ///     "vendor_dir": "/srv/probe/vendor",
    ///     "bin_dir": "/srv/probe/vendor/bin",
    ///     "dev_mode": true
    ///   }
    /// }
    /// ```
    #[rustfmt::skip]
    pub(super) fn call(&self, callback: &str, output: &impl ScriptOutput) -> Result<()> {
        output.info(&format!("> {callback}"));

        let (class, method) = callback
            .split_once("::")
            .context("invalid PHP callback: expected 'Vendor\\Class::method'")?;

        let bootstrap = include_str!("../stubs/callback_bootstrap.php")
            .replace("{class}", class)
            .replace("{method}", method);

        let callback = std::env::temp_dir().join(format!(
            "phast_cb_{}.php",
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::write(&callback, bootstrap)
            .context("failed to write PHP callback script")?;

        let ctx_path = callback.with_extension("json");
        let ctx_json = serde_json::json!({
            "project":    self.environment.project,
            "invocation": self.invocation,
        });
        std::fs::write(&ctx_path, ctx_json.to_string())
            .context("failed to write PHP callback context")?;

        let status = self.environment.command(&self.environment.php)
            .arg(&callback)
            .arg(&ctx_path)
            .status()
            .context(hints::shell_start_failed())?;

        let _ = std::fs::remove_file(&callback);
        let _ = std::fs::remove_file(&ctx_path);

        if !status.success() {
            bail!(hints::script_failed(status));
        }
        Ok(())
    }
}
