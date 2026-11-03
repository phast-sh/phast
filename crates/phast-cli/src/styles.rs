//! Named [`Style`] constants for Phast CLI output.
//!
//! The names describe a role, not a color, so two roles that render alike stay separate
//! and can diverge without touching anything that uses them.

use console::Style;

// Key/value fields: used for labeled detail views (e.g. `show vendor/pkg`).
pub const DEFAULT_KEY: Style = Style::new().cyan();
pub const DEFAULT_VALUE: Style = Style::new().yellow();
pub const DEFAULT_HINT: Style = Style::new().dim();

// Package listing: used in commands that display rows of vendor/package name + version.
pub const PACKAGE_NAME: Style = Style::new().cyan();
pub const PACKAGE_VERSION: Style = Style::new().yellow();
pub const PACKAGE_HINT: Style = Style::new().dim();

// Update kind badges: used to classify how far behind a package is.
pub const UPDATE_MAJOR: Style = Style::new().red().bold();
pub const UPDATE_MINOR: Style = Style::new().red().dim();
pub const UPDATE_PATCH: Style = Style::new().dim();

// Advisory output: used in audit text rendering.
pub const ADVISORY_ID: Style = Style::new().bold();

// Advisory severity: used in audit.
pub const SEVERITY_CRITICAL: Style = Style::new().red().bold();
pub const SEVERITY_HIGH: Style = Style::new().red();
pub const SEVERITY_MEDIUM: Style = Style::new().yellow();
pub const SEVERITY_LOW: Style = Style::new().green();
pub const SEVERITY_UNKNOWN: Style = Style::new().dim();

// Verbosity output: used for success/warning/error/hint messages.
pub const VERBOSITY_SUCCESS: Style = Style::new().green();
pub const VERBOSITY_WARNING: Style = Style::new().yellow();
pub const VERBOSITY_ERROR: Style = Style::new().red().bold();
pub const VERBOSITY_HINT: Style = Style::new().dim();
