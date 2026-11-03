//! Lifecycle hooks and named tasks from the `[scripts]` section.

use serde::{Deserialize, Serialize};

/// A single command, or a list of commands run in order.
///
/// A command is a shell line, an `@php` invocation, or a `Vendor\Class::method` callback.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Script {
    /// One command.
    Single(String),
    /// Commands run in the order given.
    Multiple(Vec<String>),
}

#[rustfmt::skip]
impl Script {
    /// The command strings in execution order, unclassified.
    pub fn as_str(&self) -> Vec<&str> {
        match self {
            Self::Single(s)   => vec![s.as_str()],
            Self::Multiple(m) => m.iter().map(String::as_str).collect(),
        }
    }
}
