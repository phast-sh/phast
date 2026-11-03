use std::io::{Cursor, Write};
use std::sync::{Arc, Mutex};

use phast_core::lock::phast::{DistSection, DistType, PackageDef};
use phast_registry::http::cache::Cache;
use phast_registry::http::client::HttpClient;
use phast_registry::http::downloader::Downloader;

use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A zip shaped like a Packagist archive: one wrapping directory holding a marker.
fn archive() -> Vec<u8> {
    let options = zip::write::SimpleFileOptions::default();
    let mut buf = Vec::new();
    let mut writer = zip::ZipWriter::new(Cursor::new(&mut buf));
    writer
        .start_file("acme-widget-deadbeef/marker.txt", options)
        .unwrap();
    writer.write_all(b"ok").unwrap();
    writer.finish().unwrap();
    buf
}

/// The package `name` at 1.0.0, carrying `dist` as given.
fn package(name: &str, dist: Option<DistSection>) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        dist,
        ..Default::default()
    }
}

/// A zip `dist` at `url`, which is what the downloader will actually fetch.
fn zip(url: &str) -> Option<DistSection> {
    Some(DistSection {
        dist_type: DistType::Zip,
        url: url.to_owned(),
        reference: Some("deadbeef".to_owned()),
        shasum: None,
    })
}

// ---------------------------------------------------------------------------
// Which packages are fetched at all
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_package_without_a_zip_dist_is_passed_over() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(archive()))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let vendor = tmp.path().join("vendor");
    let packages = vec![
        package("acme/widget", zip(&format!("{}/archive.zip", server.uri()))),
        // A workspace member: no dist to fetch.
        package("acme/member", None),
        // A path package: materialized by symlink elsewhere, not downloaded.
        package(
            "acme/local",
            Some(DistSection {
                dist_type: DistType::Path,
                url: "../packages/local".to_owned(),
                reference: None,
                shasum: None,
            }),
        ),
    ];

    Downloader::new(
        HttpClient::new().unwrap(),
        Cache::new(tmp.path().join("cache")),
    )
    .download(&packages, &vendor, |_, _| {})
    .await
    .unwrap();

    assert!(vendor.join("acme/widget/marker.txt").exists());
    assert!(!vendor.join("acme/member").exists());
    assert!(!vendor.join("acme/local").exists());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn progress_reports_each_package_that_was_extracted() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(archive()))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let packages = vec![
        package("acme/widget", zip(&format!("{}/archive.zip", server.uri()))),
        package("acme/member", None),
    ];

    let vendor = tmp.path().join("vendor");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);

    Downloader::new(
        HttpClient::new().unwrap(),
        Cache::new(tmp.path().join("cache")),
    )
    .download(&packages, &vendor, move |name, version| {
        sink.lock().unwrap().push(format!("{name}@{version}"));
    })
    .await
    .unwrap();

    // Name and version, and nothing for the package that was passed over, so a
    // caller counting callbacks is counting what actually landed in `vendor/`.
    assert_eq!(*seen.lock().unwrap(), vec!["acme/widget@1.0.0"]);
}

// ---------------------------------------------------------------------------
// Reusing what the cache already holds
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_cached_archive_is_not_fetched_again() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(archive()))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let cache = Cache::new(tmp.path().join("cache"));
    let packages = vec![package(
        "acme/widget",
        zip(&format!("{}/archive.zip", server.uri())),
    )];

    let download = async |vendor: &str| {
        Downloader::new(HttpClient::new().unwrap(), cache.clone())
            .download(&packages, &tmp.path().join(vendor), |_, _| {})
            .await
            .unwrap();
    };

    download("vendor").await;
    download("vendor-again").await;

    // A key names fixed content, so the second install extracts the same archive
    // from disk instead of asking the registry for it a second time.
    let extracted = tmp.path().join("vendor-again/acme/widget/marker.txt");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    assert!(extracted.exists());
}

#[tokio::test]
async fn corrupt_cached_archive_is_rejected_and_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join("cache");
    let vendor = tmp.path().join("vendor");

    // Seed the archive cache with junk at the shasum-keyed path: a tampered or
    // truncated entry. Its real hash will not match the expected shasum.
    let cache = Cache::new(&cache_root);
    let shasum = "0000000000000000000000000000000000000000";
    let archive_path = cache.archive_path(shasum);
    std::fs::create_dir_all(archive_path.parent().unwrap()).unwrap();
    std::fs::write(&archive_path, b"not a real zip, will not match the shasum").unwrap();

    let package = package(
        "acme/widget",
        Some(DistSection {
            dist_type: DistType::Zip,
            // Never fetched: the archive is already cached (a cache hit).
            url: "https://example.com/acme.zip".to_owned(),
            reference: None,
            shasum: Some(shasum.to_owned()),
        }),
    );

    let downloader = Downloader::new(HttpClient::new().unwrap(), cache);
    let result = downloader.download(&[package], &vendor, |_, _| {}).await;

    let err = result.expect_err("corrupt cache entry must fail verification");
    assert!(
        err.to_string().contains("checksum mismatch"),
        "unexpected error: {err}",
    );
    assert!(
        !archive_path.exists(),
        "corrupt archive should have been removed to self-heal the cache",
    );
    assert!(
        !vendor.join("acme/widget").exists(),
        "corrupt archive must never be extracted into vendor/",
    );
}

// ---------------------------------------------------------------------------
// Retrying a download that failed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn download_recovers_from_a_transient_failure() {
    let server = MockServer::start().await;

    // The first two attempts fail transiently; the third returns the archive.
    // Without retry, the first 400 would abort install.
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(400))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(archive()))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let vendor = tmp.path().join("vendor");
    let packages = vec![package(
        "acme/widget",
        zip(&format!("{}/archive.zip", server.uri())),
    )];

    Downloader::new(
        HttpClient::new().unwrap(),
        Cache::new(tmp.path().join("cache")),
    )
    .download(&packages, &vendor, |_, _| {})
    .await
    .expect("retry should recover from the transient failures");

    assert!(
        vendor.join("acme/widget/marker.txt").exists(),
        "the archive should have been fetched and extracted after retrying",
    );
}

#[tokio::test]
async fn download_fails_after_exhausting_retries() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(400))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let vendor = tmp.path().join("vendor");
    let packages = vec![package(
        "acme/widget",
        zip(&format!("{}/archive.zip", server.uri())),
    )];

    let result = Downloader::new(
        HttpClient::new().unwrap(),
        Cache::new(tmp.path().join("cache")),
    )
    .download(&packages, &vendor, |_, _| {})
    .await;

    assert!(
        result.is_err(),
        "a persistently failing download must error, not hang or silently pass",
    );
    assert!(
        !vendor.join("acme/widget").exists(),
        "nothing should have been extracted",
    );
}
