//! Build-time project metadata, derived from Cargo at compile time.

/// Base URL under which versioned `phast.toml` schemas are published.
pub const SCHEMA_BASE_URL: &str = "https://phast.sh/schema";

/// Version of the `phast.toml` JSON Schema, bumped on a breaking schema change.
///
/// Encoded in the schema path and its published URL ([`schema_url`]) so
/// multiple versions can be served side by side, independent of the crate version.
pub const SCHEMA_VERSION: &str = "1";

/// Published URL of the `phast.toml` JSON Schema for the current [`SCHEMA_VERSION`].
///
/// Served verbatim from the committed `schema/v{version}/manifest.json`.
/// Written as the schema's `$id` and as the `#:schema` directive atop generated manifests.
pub fn schema_url() -> String {
    format!("{SCHEMA_BASE_URL}/v{SCHEMA_VERSION}/manifest.json")
}

/// The project's repository URL.
///
/// Baked in at compile time from the `repository` field in `Cargo.toml`
/// (`[workspace.package]`), via the `CARGO_PKG_REPOSITORY` env var.
pub fn repository_url() -> &'static str {
    env!("CARGO_PKG_REPOSITORY")
}

/// The crate version, e.g. `0.1.0`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// `generated-by` field written into `phast.lock`.
pub fn generated_by() -> String {
    format!("phast {}", version())
}

/// HTTP `User-Agent` header value sent with every registry request.
///
/// Includes `PHAST_HTTP_CONTACT` (e.g. `you@example.com`) when set,
/// formatted as `mailto=<email>` as recommended by Packagist.
pub fn user_agent() -> String {
    let pkg = format!("phast/{}", version());
    let url = repository_url();

    if let Some(contact) = std::env::var("PHAST_HTTP_CONTACT")
        .ok()
        .filter(|s| !s.is_empty())
    {
        format!("{pkg} ({url}; mailto={contact})")
    } else {
        format!("{pkg} ({url})")
    }
}
