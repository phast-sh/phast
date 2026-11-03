//! The `[config]` section: resolver/platform settings and inline auth.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use phast_version::Stability;

/// The `[config]` / `"config"` section of a manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Lowest stability accepted during resolution, e.g. `stable` or `dev`.
    pub minimum_stability: Stability,
    /// Minimum age a package version must have before it can be selected.
    ///
    /// Versions published more recently than this threshold are excluded from
    /// resolution. `None` means no age restriction (default).
    pub minimum_package_age: Option<Duration>,
    /// When `true`, prefer stable releases over pre-releases of equal constraint fit.
    pub prefer_stable: bool,
    /// When `true`, generate the autoloader in classmap-authoritative mode: the
    /// classmap is trusted exclusively and the PSR filesystem fallback is
    /// dropped. For immutable production trees.
    pub classmap_authoritative: bool,
    /// Platform overrides: `{"php": "8.2.0", "ext-json": "1.0"}`.
    pub platform: BTreeMap<String, String>,
    /// Whether to generate the `platform_check.php` bootstrap guard,
    /// and how much of the platform it verifies.
    pub platform_check: PlatformCheck,
    /// Inline HTTP basic auth credentials from the manifest config section.
    /// Keyed by hostname. Lower priority than any auth file.
    pub http_basic: HashMap<String, BasicAuth>,
    /// Inline bearer tokens from the manifest config section.
    /// Keyed by hostname. Lower priority than any auth file.
    pub bearer: HashMap<String, String>,
    /// When `true` (the default), reject plaintext `http://` registry URLs and
    /// never send credentials over them. Loopback hosts are always exempt.
    pub secure_http: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            minimum_stability: Stability::default(),
            minimum_package_age: None,
            prefer_stable: false,
            classmap_authoritative: false,
            platform: BTreeMap::new(),
            platform_check: PlatformCheck::default(),
            http_basic: HashMap::new(),
            bearer: HashMap::new(),
            secure_http: true,
        }
    }
}

/// Whether `platform_check.php` is generated, and how much of the platform it
/// verifies at bootstrap. Read from the `platform-check` config value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlatformCheck {
    /// Verify the php version only. The default, matching Composer.
    #[default]
    Php,
    /// Verify the php version and every required extension.
    All,
    /// Do not generate `platform_check.php`.
    Off,
}

/// HTTP basic auth credential pair stored inline in a manifest config section.
#[derive(Debug, Clone, PartialEq)]
pub struct BasicAuth {
    /// Login username.
    pub username: String,
    /// Login password or personal access token.
    pub password: String,
}
