use std::path::PathBuf;

use phast_core::lock::phast::{DistSection, DistType};
use phast_core::package::PackageName;
use phast_registry::http::cache::{Cache, MetadataKind, dist_key};

use tempfile::TempDir;

/// A zip dist at `url`, carrying `reference` and `shasum` as given.
fn dist(url: &str, reference: Option<&str>, shasum: Option<&str>) -> DistSection {
    DistSection {
        dist_type: DistType::Zip,
        url: url.to_owned(),
        reference: reference.map(str::to_owned),
        shasum: shasum.map(str::to_owned),
    }
}

// ---------------------------------------------------------------------------
// Where an entry lands on disk
// ---------------------------------------------------------------------------

#[test]
fn metadata_path_layout() {
    let cache = Cache::new("/cache");
    let name = PackageName::new("vendor/package");
    assert_eq!(
        cache.metadata_path(&name, MetadataKind::Tagged),
        PathBuf::from("/cache/metadata/vendor/package.json"),
    );
    assert_eq!(
        cache.metadata_path(&name, MetadataKind::Dev),
        PathBuf::from("/cache/metadata/vendor/package~dev.json"),
    );
}

#[test]
fn etag_path_layout() {
    let cache = Cache::new("/cache");
    let name = PackageName::new("vendor/package");
    assert_eq!(
        cache.etag_path(&name, MetadataKind::Tagged),
        PathBuf::from("/cache/metadata/vendor/package.etag"),
    );
    assert_eq!(
        cache.etag_path(&name, MetadataKind::Dev),
        PathBuf::from("/cache/metadata/vendor/package~dev.etag"),
    );
}

#[test]
fn archive_path_layout() {
    let cache = Cache::new("/cache");
    let path = cache.archive_path("deadbeef");
    assert_eq!(path, PathBuf::from("/cache/archives/deadbeef.zip"));
}

#[test]
fn a_host_scopes_metadata_but_not_archives() {
    let cache = Cache::new("/cache").with_host("repo.example.com");
    let name = PackageName::new("vendor/package");

    // Metadata is per-registry so an unauthenticated public answer cannot
    // overwrite a private registry's reply for the same package.
    assert_eq!(
        cache.metadata_path(&name, MetadataKind::Tagged),
        PathBuf::from("/cache/metadata/repo.example.com/vendor/package.json"),
    );
    assert_eq!(
        cache.etag_path(&name, MetadataKind::Dev),
        PathBuf::from("/cache/metadata/repo.example.com/vendor/package~dev.etag"),
    );

    // Archives are keyed by content rather than by who served them, so one copy
    // is meant to serve every registry and the host does not appear.
    assert_eq!(
        cache.archive_path("deadbeef"),
        PathBuf::from("/cache/archives/deadbeef.zip"),
    );
}

// ---------------------------------------------------------------------------
// Which key an archive is cached under
// ---------------------------------------------------------------------------

#[test]
fn dist_key_prefers_the_checksum_then_the_reference() {
    // The checksum is a content address, so it is the key that lets one download
    // serve every project resolving that same package version.
    assert_eq!(
        dist_key(&dist("https://x/y.zip", Some("ref123"), Some("sha456"))).as_deref(),
        Some("sha456"),
    );
    assert_eq!(
        dist_key(&dist("https://x/y.zip", Some("ref123"), None)).as_deref(),
        Some("ref123"),
    );
}

#[test]
fn dist_key_skips_a_field_that_is_present_but_empty() {
    // A registry that sends the key with an empty value is saying it has none,
    // which is not the same as naming the empty string.
    assert_eq!(
        dist_key(&dist("https://x/y.zip", Some("ref123"), Some(""))).as_deref(),
        Some("ref123"),
    );
    assert!(
        dist_key(&dist("https://x/y.zip", Some(""), Some("")))
            .is_some_and(|key| key.starts_with("url-")),
    );
}

#[test]
fn dist_key_falls_back_to_hashing_the_url() {
    let key = dist_key(&dist("https://x/y.zip", None, None)).unwrap();

    // The last resort for a v1 Satis registry that publishes neither field. It
    // is derived from the URL, so it is stable for one and differs across two.
    assert!(key.starts_with("url-"), "{key}");
    assert_eq!(dist_key(&dist("https://x/y.zip", None, None)).unwrap(), key);
    assert_ne!(
        dist_key(&dist("https://x/other.zip", None, None)).unwrap(),
        key,
    );
}

#[test]
fn dist_key_is_none_when_there_is_nothing_to_key_on() {
    assert!(dist_key(&dist("", None, None)).is_none());
    assert!(dist_key(&dist("", Some(""), Some(""))).is_none());
}

// ---------------------------------------------------------------------------
// Reading back what was written
// ---------------------------------------------------------------------------

#[test]
fn read_metadata_returns_none_when_absent() {
    let tmp = TempDir::new().unwrap();
    let cache = Cache::new(tmp.path());
    let name = PackageName::new("vendor/pkg");
    assert!(cache.read_metadata(&name, MetadataKind::Tagged).is_none());
}

#[test]
fn read_etag_returns_none_when_absent() {
    let tmp = TempDir::new().unwrap();
    let cache = Cache::new(tmp.path());
    let name = PackageName::new("vendor/pkg");
    assert!(cache.read_etag(&name, MetadataKind::Tagged).is_none());
}

#[test]
fn write_and_read_metadata_round_trip() {
    let tmp = TempDir::new().unwrap();
    let cache = Cache::new(tmp.path());
    let name = PackageName::new("vendor/pkg");
    cache.write_metadata(
        &name,
        MetadataKind::Tagged,
        r#"{"packages":{}}"#,
        Some("\"etag-abc\""),
    );
    assert_eq!(
        cache.read_metadata(&name, MetadataKind::Tagged).as_deref(),
        Some(r#"{"packages":{}}"#),
    );
    assert_eq!(
        cache.read_etag(&name, MetadataKind::Tagged).as_deref(),
        Some("\"etag-abc\""),
    );
}

#[test]
fn write_metadata_without_etag_skips_sidecar() {
    let tmp = TempDir::new().unwrap();
    let cache = Cache::new(tmp.path());
    let name = PackageName::new("vendor/pkg");
    cache.write_metadata(&name, MetadataKind::Tagged, "body", None);
    assert_eq!(
        cache.read_metadata(&name, MetadataKind::Tagged).as_deref(),
        Some("body"),
    );
    assert!(cache.read_etag(&name, MetadataKind::Tagged).is_none());
}

#[test]
fn tagged_and_dev_kinds_cache_independently() {
    let tmp = TempDir::new().unwrap();
    let cache = Cache::new(tmp.path());
    let name = PackageName::new("vendor/pkg");

    // The two p2 resources for one package must occupy separate slots
    // so their bodies and ETags survive independently across a resolve.
    cache.write_metadata(&name, MetadataKind::Tagged, "tagged-body", Some("\"tag\""));
    cache.write_metadata(&name, MetadataKind::Dev, "dev-body", Some("\"dev\""));

    assert_eq!(
        cache.read_metadata(&name, MetadataKind::Tagged).as_deref(),
        Some("tagged-body"),
    );
    assert_eq!(
        cache.read_metadata(&name, MetadataKind::Dev).as_deref(),
        Some("dev-body"),
    );
    assert_eq!(
        cache.read_etag(&name, MetadataKind::Tagged).as_deref(),
        Some("\"tag\""),
    );
    assert_eq!(
        cache.read_etag(&name, MetadataKind::Dev).as_deref(),
        Some("\"dev\""),
    );
}
