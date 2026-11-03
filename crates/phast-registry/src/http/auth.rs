//! Authentication for private registries.
//!
//! ## Credential files
//!
//! Phast loads credentials from up to four files, in priority order (highest first):
//!
//! 1. `./auth.toml`           project-local, Phast-native
//! 2. `./auth.json`           project-local, Composer-compatible
//! 3. `~/.phast/auth.toml`    global Phast
//! 4. `~/.composer/auth.json` global Composer
//!
//! Higher-priority sources win on a per-hostname basis. This means a developer's
//! local `auth.toml` always overrides anything in the global files.
//!
//! Credentials declared inline in a manifest's config section rank below every file,
//! so a checked-in manifest cannot override what a developer put in their own `auth.toml`.
//!
//! ## Supported auth methods
//!
//! - Bearer token (GitHub Packages, Private Packagist API tokens)
//! - Basic auth (username + password)
//! - GitHub PAT (stored under `"github-token"` key)
//! - GitLab token (stored under `"gitlab-token"`)
//!
//! ## auth.toml format
//!
//! ```toml
//! [http-basic]
//! "repo.example.com" = { username = "you@example.com", password = "secret" }
//!
//! [bearer]
//! "packages.example.com" = "token"
//! ```
//!
//! ## auth.json format
//!
//! ```json
//! {
//!   "http-basic": { "repo.example.com": { "username": "user", "password": "pass" } },
//!   "bearer": { "packages.example.com": "token" }
//! }
//! ```
//!
//! Neither example is exhaustive: `github-token` and `gitlab-token` take the same
//! host-keyed shape as `bearer`.

use std::collections::HashMap;
use std::fs::read_to_string;
use std::path::Path;

use phast_core::manifest::BasicAuth as ManifestBasicAuth;
use phast_core::manifest::Config;
use phast_core::manifest::composer::ComposerConfig;

use reqwest::RequestBuilder;
use serde::{Deserialize, Serialize};

/// Credentials loaded from auth files and/or the manifest config section.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AuthConfig {
    /// HTTP basic auth credentials keyed by hostname.
    #[serde(
        rename = "http-basic",
        default,
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub http_basic: HashMap<String, BasicAuth>,

    /// Bearer tokens keyed by hostname.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub bearer: HashMap<String, String>,

    /// GitHub PAT tokens keyed by hostname, e.g. `"github.com"`.
    #[serde(
        rename = "github-token",
        default,
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub github_token: HashMap<String, String>,

    /// GitLab personal access tokens keyed by hostname.
    #[serde(
        rename = "gitlab-token",
        default,
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub gitlab_token: HashMap<String, String>,
}

impl AuthConfig {
    /// Load credentials from all auth files, merging them in priority order.
    ///
    /// The files and their order are listed in the [module docs](self).
    ///
    /// A file Phast cannot read is skipped in silence, whether it is absent or
    /// unreadable. One that reads but does not parse returns an error, so a
    /// malformed file is never mistaken for an empty one.
    pub fn load(cwd: &Path) -> anyhow::Result<Self> {
        let mut config = AuthConfig::default();

        // Highest-priority sources first, so the first value
        // inserted for a given hostname wins.
        //
        // Local files beat global files.
        config.merge(Self::load_from_toml_file(&cwd.join("auth.toml"))?);
        config.merge(Self::load_from_json_file(&cwd.join("auth.json"))?);

        if let Some(home) = dirs::home_dir() {
            config.merge(Self::load_from_toml_file(
                &home.join(".phast").join("auth.toml"),
            )?);
            config.merge(Self::load_from_json_file(
                &home.join(".composer").join("auth.json"),
            )?);
        }

        Ok(config)
    }

    /// Merge `other` into `self`, with `self` winning on any per-hostname conflict.
    ///
    /// Used to layer lower-priority sources (global files, manifest config) under
    /// higher-priority sources (local files).
    pub fn merge(&mut self, other: AuthConfig) {
        for (host, token) in other.bearer {
            self.bearer.entry(host).or_insert(token);
        }
        for (host, token) in other.github_token {
            self.github_token.entry(host).or_insert(token);
        }
        for (host, token) in other.gitlab_token {
            self.gitlab_token.entry(host).or_insert(token);
        }
        for (host, credentials) in other.http_basic {
            self.http_basic.entry(host).or_insert(credentials);
        }
    }

    /// Find the appropriate credential for a registry URL or bare hostname.
    ///
    /// Accepts either a full URL (`https://example.com/packages.json`) or a bare
    /// hostname (`example.com`). When a URL is given the hostname is extracted;
    /// when URL parsing yields no host the input is used as-is.
    ///
    /// Priority: bearer > github_token > gitlab_token > http_basic
    pub fn credential_for(&self, url_or_host: &str) -> Option<Credential> {
        let host = reqwest::Url::parse(url_or_host)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_else(|| url_or_host.to_owned());

        if let Some(token) = self.bearer.get(&host) {
            return Some(Credential::Bearer(token.clone()));
        }
        if let Some(token) = self.github_token_for(&host) {
            return Some(Credential::Bearer(token.clone()));
        }
        if let Some(token) = self.gitlab_token_for(&host) {
            return Some(Credential::Bearer(token.clone()));
        }
        if let Some(basic) = self.http_basic.get(&host) {
            return Some(Credential::Basic {
                username: basic.username.clone(),
                password: basic.password.clone(),
            });
        }
        None
    }

    /// Returns `true` if no credentials are stored.
    pub fn is_empty(&self) -> bool {
        self.http_basic.is_empty()
            && self.bearer.is_empty()
            && self.github_token.is_empty()
            && self.gitlab_token.is_empty()
    }

    /// Load an `AuthConfig` from a single TOML file.
    ///
    /// Returns `Ok(default)` when the file cannot be read at all, which covers
    /// an absent file and an unreadable one alike. Returns `Err` when it reads
    /// but does not parse, so a malformed file is never mistaken for an empty one.
    pub fn load_from_toml_file(path: &Path) -> anyhow::Result<Self> {
        let Ok(content) = read_to_string(path) else {
            return Ok(Self::default());
        };
        toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("failed to parse {}: {}", path.display(), e))
    }

    /// Load an `AuthConfig` from a single JSON file.
    ///
    /// Returns `Ok(default)` when the file cannot be read at all, which covers
    /// an absent file and an unreadable one alike. Returns `Err` when it reads
    /// but does not parse, so a malformed file is never mistaken for an empty one.
    pub fn load_from_json_file(path: &Path) -> anyhow::Result<Self> {
        let Ok(content) = read_to_string(path) else {
            return Ok(Self::default());
        };
        serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("failed to parse {}: {}", path.display(), e))
    }

    /// Load credentials from all auth files and merge inline credentials from `manifest`.
    ///
    /// Equivalent to calling [`load`](Self::load) followed by merging the credentials
    /// declared in the manifest `[config]` section. Use this as the single entry point
    /// when both file-based and inline credentials are needed.
    pub fn load_from_manifest(cwd: &Path, manifest: &Config) -> anyhow::Result<Self> {
        let mut auth = Self::load(cwd)?;
        auth.merge(Self::from(manifest));

        Ok(auth)
    }

    /// The GitHub token for `host`.
    ///
    /// `github-token` is conventionally keyed by `github.com`, but Phast's
    /// requests hit GitHub's API and download subdomains, so those resolve to
    /// the same token.
    fn github_token_for(&self, host: &str) -> Option<&String> {
        let key = match host {
            "api.github.com" | "raw.githubusercontent.com" | "codeload.github.com" => "github.com",
            other => other,
        };
        self.github_token.get(key)
    }

    /// The GitLab token for `host`.
    ///
    /// Unlike GitHub, GitLab serves its API, raw files, and archives all
    /// from the instance host itself (`gitlab.com/api/v4/...`), so there
    /// is no subdomain to fold and the lookup is by host directly.
    fn gitlab_token_for(&self, host: &str) -> Option<&String> {
        self.gitlab_token.get(host)
    }
}

impl From<&Config> for AuthConfig {
    fn from(config: &Config) -> Self {
        Self {
            http_basic: config
                .http_basic
                .iter()
                .map(|(host, ManifestBasicAuth { username, password })| {
                    (
                        host.clone(),
                        BasicAuth {
                            username: username.clone(),
                            password: password.clone(),
                        },
                    )
                })
                .collect(),
            bearer: config.bearer.clone(),
            ..Default::default()
        }
    }
}

impl From<&ComposerConfig> for AuthConfig {
    fn from(config: &ComposerConfig) -> Self {
        Self {
            http_basic: config
                .http_basic
                .iter()
                .map(|(host, basic)| {
                    (
                        host.clone(),
                        BasicAuth {
                            username: basic.username.clone(),
                            password: basic.password.clone(),
                        },
                    )
                })
                .collect(),
            bearer: config.bearer.clone(),
            ..Default::default()
        }
    }
}

/// HTTP basic authentication credentials for a single registry host.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BasicAuth {
    /// Login username.
    pub username: String,
    /// Login password or personal access token.
    pub password: String,
}

/// The resolved credential for a specific registry host.
pub enum Credential {
    /// A bearer token sent in the `Authorization: Bearer` header.
    Bearer(String),
    /// Username and password sent via HTTP basic authentication.
    Basic { username: String, password: String },
}

impl Credential {
    /// Apply this credential to a request builder.
    pub fn apply(&self, request: RequestBuilder) -> RequestBuilder {
        match self {
            Credential::Bearer(token) => request.bearer_auth(token),
            Credential::Basic { username, password } => {
                request.basic_auth(username, Some(password))
            }
        }
    }
}
