//! Lifecycle event names and event dispatch for script hooks.
//!
//! [`LifecycleEvent`] enumerates the reserved hook names. [`LifecycleEventManager`]
//! wires handlers to events and fires them at the appropriate points in the install
//! lifecycle. Handlers are registered by the caller: each CLI command decides which
//! events it cares about and what to do when they fire.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;

/// Composer-compatible lifecycle events that Phast recognises and fires automatically.
///
/// These correspond to reserved keys in the `[scripts]` section of `phast.toml`.
/// Custom script names (for `phast run`) are plain strings and not represented here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LifecycleEvent {
    /// Fired before the install command begins.
    PreInstallCmd,
    /// Fired after the install command completes.
    PostInstallCmd,
    /// Fired before the update command begins.
    PreUpdateCmd,
    /// Fired after the update command completes.
    PostUpdateCmd,
    /// Fired before the autoload file is regenerated.
    PreAutoloadDump,
    /// Fired after the autoload file is regenerated.
    PostAutoloadDump,
}

impl LifecycleEvent {
    /// The reserved string key as it appears in `phast.toml`.
    #[rustfmt::skip]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PreInstallCmd     => "pre-install-cmd",
            Self::PostInstallCmd    => "post-install-cmd",
            Self::PreUpdateCmd      => "pre-update-cmd",
            Self::PostUpdateCmd     => "post-update-cmd",
            Self::PreAutoloadDump   => "pre-autoload-dump",
            Self::PostAutoloadDump  => "post-autoload-dump",
        }
    }
}

/// A bound handler: a callable taking nothing, able to fail.
///
/// Shared rather than owned so the manager stays `Clone`, which is what lets
/// a command hand the same set of hooks to more than one stage.
type LifecycleEventHandler = Arc<dyn Fn() -> Result<()>>;

/// Dispatches lifecycle events to registered handlers.
///
/// Handlers are registered by the caller before the operation starts. Emitting
/// an event with no registered handlers is a silent no-op, so call sites never
/// need to guard against missing handlers.
///
/// ```
/// use phast_runtime::event::{LifecycleEventManager, LifecycleEvent};
///
/// let mut lifecycle = LifecycleEventManager::new();
/// lifecycle.bind(LifecycleEvent::PostInstallCmd, || {
///     println!("install done");
///     Ok(())
/// });
/// lifecycle.fire(LifecycleEvent::PostInstallCmd).unwrap();
/// ```
#[derive(Default, Clone)]
pub struct LifecycleEventManager {
    /// Handlers per event, kept in registration order.
    handlers: HashMap<LifecycleEvent, Vec<LifecycleEventHandler>>,
}

impl LifecycleEventManager {
    /// Create an event manager with no registered handlers.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a handler for an event. Multiple handlers per event are supported
    /// and are called in registration order.
    pub fn bind(&mut self, event: LifecycleEvent, handler: impl Fn() -> Result<()> + 'static) {
        self.handlers
            .entry(event)
            .or_default()
            .push(Arc::new(handler));
    }

    /// Fire all handlers registered for `event` in order, stopping on the first error.
    ///
    /// Returns `Ok(())` immediately when no handlers are registered for the event.
    pub fn fire(&self, event: LifecycleEvent) -> Result<()> {
        for handler in self.handlers.get(&event).into_iter().flatten() {
            handler()?;
        }
        Ok(())
    }
}
