//! Script resolution: the commands a script runs, once its `@` calls are filled in.
//!
//! A script can run another one with `@name`. Here, `@name` is replaced with that
//! script's commands, so the runner only ever gets plain commands.

use crate::hints;
use crate::runner::ScriptEntry;

use std::collections::BTreeMap;

use phast_core::manifest::Script;

use anyhow::{Result, bail};

/// The `[scripts]` of a manifest, by name.
#[derive(Debug)]
pub struct Scripts {
    scripts: BTreeMap<String, Script>,
}

#[rustfmt::skip]
impl Scripts {
    /// Take the scripts from a manifest's `[scripts]`, keyed by name.
    pub fn new(scripts: BTreeMap<String, Script>) -> Self {
        Self { scripts }
    }

    /// The commands `name` runs, or `None` when there's no script by that name.
    ///
    /// Fails if a script calls one that doesn't exist, or if scripts call each other
    /// in a cycle. Nothing has run at that point, so a broken script stops before its
    /// first command.
    pub fn resolve(&self, name: &str) -> Result<Option<Vec<ScriptEntry>>> {
        if !self.scripts.contains_key(name) {
            return Ok(None);
        }
        self.expand(name, &[]).map(Some)
    }

    /// The commands of `name`, with every `@` call replaced by the commands of the
    /// script it calls. `seen` holds the scripts that led to this one.
    fn expand(&self, name: &str, seen: &[&str]) -> Result<Vec<ScriptEntry>> {
        // Only an `@` call can name a script that doesn't exist.
        let Some(def) = self.scripts.get(name) else {
            bail!(hints::script_missing(name));
        };

        // Already on the way here means the scripts call each other in a cycle.
        if seen.contains(&name) {
            bail!(hints::script_cycle(&[seen, &[name]].concat().join(" → ")));
        }

        // A copy with this script added, so each call only sees the scripts above it.
        let seen = [seen, &[name]].concat();

        let mut commands = Vec::new();
        for script in ScriptEntry::from_def(def) {
            match script {
                ScriptEntry::Reference(target) => {
                    commands.extend(self.expand(&target, &seen)?)
                },
                script => {
                    commands.push(script)
                },
            }
        }
        Ok(commands)
    }
}
