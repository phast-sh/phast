//! Lifecycle hooks and named tasks from the `[scripts]` section.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// A single command, a list of commands run in order, or a map of commands.
///
/// A command is a shell line, an `@php` invocation, a `Vendor\Class::method` callback,
/// or `@name` to run another script.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Script {
    /// One command.
    Single(String),
    /// Commands run in the given order.
    Multiple(Vec<String>),
    /// Commands mapped to a value that says how to run each, in the written order.
    Map(IndexMap<String, String>),
}

#[rustfmt::skip]
impl Script {
    /// The command strings in execution order, unclassified.
    ///
    /// Empty for a map, since its commands are kept but not run.
    pub fn as_str(&self) -> Vec<&str> {
        match self {
            Self::Single(s)   => vec![s.as_str()],
            Self::Multiple(m) => m.iter().map(String::as_str).collect(),
            Self::Map(_)      => vec![],
        }
    }
}
