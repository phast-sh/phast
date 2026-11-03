//! The shared HTTP client, its credentials, and its transport-security guard.

use crate::hints;
use crate::http::auth::AuthConfig;

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use phast_core::meta::user_agent;

use anyhow::{Context, bail};
use reqwest::Client;
use reqwest::RequestBuilder;

/// Seconds before an in-flight HTTP request is aborted.
const REQUEST_TIMEOUT_SECS: u64 = 60;
/// Maximum idle connections kept open per host in the connection pool.
const MAX_IDLE_CONNECTIONS_PER_HOST: usize = 20;

/// An HTTP client with credentials baked in.
///
/// Wraps [`Client`] and applies the right credential for each
/// request URL automatically, so callers never need to handle auth separately.
///
/// ## Transport security (`secure-http`)
///
/// By default, only `https://` gets through, so registry tokens and passwords can't
/// leak over an unencrypted connection. Any other scheme is refused before a
/// credential is attached, which makes this an allowlist rather than a ban aimed
/// at `http://`.
///
/// Loopback hosts are exempt, because that traffic never leaves the machine. They are
/// recognized by name (`localhost`, `*.localhost`) or by literal address
/// (`127.0.0.0/8`, `::1`), never by DNS lookup. To opt out entirely, set
/// `secure-http = false` in the manifest `[config]`, the same key Composer uses and
/// likewise on by default (see [`with_secure_http`](Self::with_secure_http)).
#[derive(Clone)]
pub struct HttpClient {
    /// Shared connection pool.
    client: Arc<Client>,
    /// Credentials applied per-request based on the request URL's hostname.
    auth: AuthConfig,
    /// When `true`, reject plaintext `http://` URLs to non-loopback hosts.
    secure_http: bool,
}

impl HttpClient {
    /// Build a new client with Phast's default connection pool settings.
    ///
    /// Every request carries a timeout, so a stalled registry cannot block
    /// resolution indefinitely, and idle connections are capped per host so
    /// resolving against many registries at once does not accumulate them.
    ///
    /// `secure-http` enforcement is on by default.
    pub fn new() -> anyhow::Result<Self> {
        let client = Client::builder()
            .user_agent(user_agent())
            .gzip(true)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .pool_max_idle_per_host(MAX_IDLE_CONNECTIONS_PER_HOST)
            .http2_adaptive_window(true)
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {}", e))?;

        Ok(Self {
            client: Arc::new(client),
            auth: AuthConfig::default(),
            secure_http: true,
        })
    }

    /// Attach credentials to this client.
    pub fn with_auth(mut self, auth: AuthConfig) -> Self {
        self.auth = auth;
        self
    }

    /// Enable or disable `secure-http` enforcement.
    ///
    /// Passing `false` allows plaintext `http://` registry URLs and lets
    /// credentials be sent over them. Only do this for trusted internal
    /// registries; it exposes tokens to anyone on the network path.
    pub fn with_secure_http(mut self, secure_http: bool) -> Self {
        self.secure_http = secure_http;
        self
    }

    /// Begin a GET request, applying the matching credential for the URL's hostname.
    ///
    /// Returns an error when `secure-http` enforcement refuses `url`.
    pub fn get(&self, url: &str) -> anyhow::Result<RequestBuilder> {
        self.guard_scheme(url)?;
        Ok(self.with_credentials(url, self.client.get(url)))
    }

    /// Begin a POST request, applying the matching credential for the URL's hostname.
    ///
    /// Returns an error when `secure-http` enforcement refuses `url`.
    pub fn post(&self, url: &str) -> anyhow::Result<RequestBuilder> {
        self.guard_scheme(url)?;
        Ok(self.with_credentials(url, self.client.post(url)))
    }

    /// Attach the matching credential for `url`, if any.
    ///
    /// Only reached after [`guard_scheme`](Self::guard_scheme) has accepted the
    /// URL, so a credential is never applied to a blocked plaintext request.
    fn with_credentials(&self, url: &str, request: RequestBuilder) -> RequestBuilder {
        match self.auth.credential_for(url) {
            Some(c) => c.apply(request),
            None => request,
        }
    }

    /// Accept only `https://` and loopback hosts when `secure-http` is enabled,
    /// rejecting everything else, a malformed URL included.
    fn guard_scheme(&self, url: &str) -> anyhow::Result<()> {
        if !self.secure_http {
            return Ok(());
        }

        let parsed = reqwest::Url::parse(url).with_context(|| format!("invalid URL: {url}"))?;
        if parsed.scheme() == "https" {
            return Ok(());
        }
        if parsed.host_str().is_some_and(is_loopback_host) {
            return Ok(());
        }

        bail!(hints::insecure_http(url));
    }
}

/// Return `true` for hosts that name the local machine.
///
/// Plaintext traffic to a loopback host never crosses the network, so it is
/// exempt from `secure-http` enforcement. Matching is syntactic: `localhost`,
/// the `*.localhost` reserved suffix, and loopback IP literals (`127.0.0.0/8`,
/// `::1`). A name that only reaches 127.0.0.1 through DNS is not exempt.
fn is_loopback_host(host: &str) -> bool {
    // `Url::host_str` keeps IPv6 literals bracketed (`[::1]`); strip them so the
    // address parses.
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);

    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }

    host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}
