//! Script entry classification, expansion, and orchestration.
//!
//! `ScriptEntry` classifies a raw manifest string into an execution variant.
//! `ScriptRunner` owns the scripts map and an `Executor`, and is the public
//! entry point for running lifecycle hooks.
//!
//! ## Classification order
//!
//! 1. `@php ` prefix  → `PhpCommand`
//! 2. any other `@`   → `Reference`
//! 3. contains `::`   → `PhpCallback`
//! 4. everything else → `Shell`
//!
//! The order matters, `@php` is checked first, so `@php Foo::bar` runs PHP
//! instead of calling `Foo::bar`.

use crate::context::Invocation;
use crate::executor::{Executor, ScriptOutput};
use crate::scripts::Scripts;

use std::collections::BTreeMap;
use std::str::FromStr;

use phast_core::manifest::Script;

use anyhow::Result;

/// Looks up scripts by name and delegates execution to `Executor`.
#[derive(Debug)]
pub struct ScriptRunner {
    scripts: Scripts,
    executor: Executor,
}

#[rustfmt::skip]
impl ScriptRunner {
    /// Create a `ScriptRunner` with the given scripts map and executor.
    pub fn new(scripts: BTreeMap<String, Script>, executor: Executor) -> Self {
        Self { scripts: Scripts::new(scripts), executor }
    }

    /// Run a lifecycle hook, doing nothing when the manifest defines no script
    /// under that name. Every event fires on every run, so most of them find
    /// nothing, and that is the ordinary case rather than a failure.
    ///
    /// ```toml
    /// [scripts]
    /// post-install-cmd = ["@php artisan migrate", "echo done"]
    /// ```
    pub fn run(&self, invocation: &Invocation, output: &impl ScriptOutput) -> Result<()> {
        let Some(scripts) = self.scripts.resolve(&invocation.event)? else {
            return Ok(());
        };

        for script in scripts {
            self.executor.execute(&script, output)?;
        }
        Ok(())
    }

    /// Run a named user script from `[scripts]`, appending the invocation's
    /// `arguments` to the last command in the sequence.
    ///
    /// ```toml
    /// [scripts]
    /// test = ["@php vendor/bin/phpcs src/", "@php vendor/bin/phpunit"]
    /// ```
    ///
    /// Extra args are appended only to `Shell` and `PhpCommand` entries. Passing
    /// extra args alongside a `PhpCallback` entry has no effect: callbacks are
    /// static method calls and do not accept CLI arguments.
    ///
    /// An unknown name is not an error here; the caller decides what to say about
    /// a script the user asked for and the manifest does not define.
    pub fn run_named(&self, invocation: &Invocation, output: &impl ScriptOutput) -> Result<()> {
        let Some(mut scripts) = self.scripts.resolve(&invocation.event)? else {
            return Ok(());
        };

        // Append the arguments to the last entry, if any were supplied.
        if !invocation.arguments.is_empty()
            && let Some(script) = scripts.last_mut() {
                let extra = invocation.arguments.join(" ");
                *script = match script {
                    ScriptEntry::Shell(cmd) => {
                        ScriptEntry::Shell(format!("{cmd} {extra}"))
                    },
                    ScriptEntry::PhpCommand(args) => {
                        ScriptEntry::PhpCommand(format!("{args} {extra}"))
                    },
                    _ => script.clone(),
                };
        }

        for script in &scripts {
            self.executor.execute(script, output)?;
        }
        Ok(())
    }
}

/// A single executable script entry, classified from a raw manifest string.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptEntry {
    /// A direct PHP invocation prefixed with `@php`, e.g. `@php artisan migrate`.
    /// The `@php` prefix is stripped; the remainder is passed as PHP arguments.
    PhpCommand(String),
    /// A PHP static callback in `Vendor\Class::method` form.
    PhpCallback(String),
    /// A shell command passed directly to the system shell.
    Shell(String),
    /// Another script from the same manifest, written as `@name`.
    Reference(String),
}

/// Classifies a raw manifest string into the appropriate `ScriptEntry` variant.
impl FromStr for ScriptEntry {
    type Err = std::convert::Infallible;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        if let Some(cmd) = string.strip_prefix("@php ") {
            return Ok(Self::PhpCommand(cmd.to_owned()));
        }
        if let Some(name) = string.strip_prefix('@') {
            return Ok(Self::Reference(name.to_owned()));
        }
        if string.contains("::") {
            return Ok(Self::PhpCallback(string.to_owned()));
        }
        Ok(Self::Shell(string.to_owned()))
    }
}

/// Constructors for building entries from manifest definitions.
impl ScriptEntry {
    /// Classify every command a `[scripts]` entry holds, in declaration order.
    ///
    /// A single string yields one entry and a list yields one per element,
    /// so a caller runs the result as a sequence either way.
    pub fn from_def(def: &Script) -> Vec<Self> {
        def.as_str()
            .into_iter()
            .map(|s| s.parse().unwrap())
            .collect()
    }
}
