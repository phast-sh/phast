//! A mock Packagist-shaped registry, for tests that must not touch the network.
//!
//! [`MockRegistry`] wraps a [`MockServer`] and mounts the three endpoints an install
//! walks through: `/packages.json` for discovery, `/p2/{package}.json` for metadata,
//! and a dist URL per package serving a zip archive.
//!
//! Pass the mounted server's [`uri`](MockRegistry::uri) to the binary with
//! `--registry-url` so resolution and download both stay local.

use std::io::Cursor;
use std::io::Write;

use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// A running mock registry with the discovery endpoint already mounted.
pub struct MockRegistry {
    server: MockServer,
}

impl MockRegistry {
    /// Base URL to hand the binary via `--registry-url`.
    pub fn uri(&self) -> String {
        self.server.uri()
    }

    /// Start a server and mount `/packages.json`, pointing metadata lookups at `/p2/`.
    pub async fn start() -> Self {
        let server = MockServer::start().await;

        let body = json!({
            "metadata-url": "/p2/%package%.json",
        });

        Mock::given(method("GET"))
            .and(path("/packages.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        Self { server }
    }

    /// Mount one package: a metadata document listing `versions`, and an archive each.
    ///
    /// `name` is a full package name such as `vendor/acme`. `metadata` is written into
    /// every version entry, which is how a caller declares `require` to build a dependency
    /// chain, or `provide` and `replace` to make one package stand in for another.
    ///
    /// All of a package's versions belong to one p2 response, so they are declared
    /// together rather than by calling this twice: a second registration on the same
    /// path would sit behind the first and never be served.
    pub async fn mount(&self, name: &str, versions: &[&str], metadata: Value) {
        let versions: Vec<(&str, Value)> = versions
            .iter()
            .map(|version| (*version, metadata.clone()))
            .collect();

        self.mount_each(name, &versions).await;
    }

    /// [`mount`](Self::mount), with metadata of its own for each version, such as a `time`
    /// that puts one release inside `minimum-package-age` and another outside it.
    #[rustfmt::skip]
    pub async fn mount_each(&self, name: &str, versions: &[(&str, Value)]) {
        let packages: Vec<(MockPackage, &Value)> = versions
            .iter()
            .map(|(version, metadata)| (MockPackage::new(name, version, &self.uri()), metadata))
            .collect();

        let versions = packages
            .iter()
            .map(|(package, metadata)| package.version(metadata))
            .collect();

        Mock::given(method("GET"))
            .and(path(format!("/p2/{name}.json")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(MockPackage::metadata(name, versions))
            )
            .mount(&self.server)
            .await;

        for (package, _) in &packages {
            Mock::given(method("GET"))
                .and(path(package.dist_path.as_str()))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_bytes(package.archive()),
                )
                .mount(&self.server)
                .await;
        }
    }

    /// How many requests the server has received.
    ///
    /// Zero is the interesting answer: it is how a test asserts that a command decided
    /// something without asking a registry, which an unreachable address cannot prove
    /// because a failed lookup is indistinguishable from an empty one.
    pub async fn requests(&self) -> usize {
        self.server
            .received_requests()
            .await
            .map_or(0, |requests| requests.len())
    }
}

/// One package version.
struct MockPackage {
    /// Full package name, e.g. `vendor/acme`.
    name: String,
    /// Version string as written in the metadata, e.g. `1.0.0`.
    version: String,
    /// Stand-in for the commit hash Packagist serves, also the archive's root directory.
    reference: String,
    /// Server-relative path the archive is mounted at.
    dist_path: String,
    /// Absolute URL to `dist_path`, as it appears in the metadata document.
    dist_url: String,
}

#[rustfmt::skip]
impl MockPackage {
    /// Derive every path for `name` at `version`, with archives served from `base_uri`.
    fn new(name: &str, version: &str, base_uri: &str) -> Self {
        let reference = format!("{name}{version}ref").replace(['/', '.', '-'], "");
        let dist_path = format!("/dist/{name}-{version}.zip");
        let dist_url  = format!("{base_uri}{dist_path}");

        Self {
            name: name.to_string(),
            version: version.to_string(),
            reference,
            dist_path,
            dist_url,
        }
    }

    /// The p2 metadata document: a `packages` map keyed by name, each holding a list
    /// of versions, as Packagist serves from its `metadata-url` endpoint.
    fn metadata(name: &str, versions: Vec<Value>) -> Value {
        json!({ "packages": { name: versions } })
    }

    /// One version entry, as it appears in the list under a package name.
    ///
    /// `metadata` is merged over the defaults below, so a caller can add `provide` or
    /// override `type` without restating what every package has.
    fn version(&self, metadata: &Value) -> Value {
        let Self { name, version, reference, dist_url, .. } = self;

        let (vendor, package) = name
            .split_once('/')
            .unwrap_or_else(|| panic!("package name must be vendor/package, got {name}"));

        let namespace = format!("{}\\{}\\",
            capitalize(vendor),
            capitalize(package),
        );

        let mut json = json!({
            "version": version,
            "version-normalized": normalize(version),
            "type": "library",
            "dist": {
                "type": "zip",
                "url": dist_url,
                "reference": reference,
                "shasum": ""
            },
            "autoload": {
                "psr-4": {
                    namespace: "src/"
                }
            },
        });

        let (
            Some(json),
            Some(metadata),
        ) = (
            json.as_object_mut(),
            metadata.as_object(),
        ) else {
            panic!("package metadata must be a JSON object, got {metadata}");
        };

        for (key, value) in metadata {
            json.insert(key.clone(), value.clone());
        }

        Value::Object(json.clone())
    }

    /// Build a minimal archive in the layout Packagist serves.
    ///
    /// Every entry sits under a single root directory named
    /// `{vendor}-{package}-{reference}`, which the extractor strips. A `composer.json`
    /// is included so the extracted directory is non-empty and reads as a populated
    /// install.
    fn archive(&self) -> Vec<u8> {
        let Self { name, version, reference, .. } = self;

        let prefix  = format!("{}-{reference}/", name.replace('/', "-"));
        let options = SimpleFileOptions::default();

        let mut buf = Vec::new();
        let mut zip = ZipWriter::new(Cursor::new(&mut buf));

        zip
            .start_file(format!("{prefix}composer.json"), options)
            .expect("failed to start composer.json entry");
        zip
            .write_all(
                json!({ "name": name, "version": version })
                    .to_string()
                    .as_bytes(),
            )
            .expect("failed to write composer.json entry");
        zip
            .finish()
            .expect("failed to finish archive");

        buf
    }
}

/// Expand a `major.minor.patch` version to the four-segment normalized form.
fn normalize(version: &str) -> String {
    let mut segments: Vec<&str> = version.split('.').collect();
    while segments.len() < 4 {
        segments.push("0");
    }
    segments.join(".")
}

/// Upper-case the first character, for deriving a PSR-4 namespace from a package name.
fn capitalize(segment: &str) -> String {
    let mut chars = segment.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
