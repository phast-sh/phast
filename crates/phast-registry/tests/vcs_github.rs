use phast_registry::http::client::HttpClient;
use phast_registry::vcs::github::GitHubHost;
use phast_registry::vcs::{Host, RefKind, VcsHost, host_for, manifest_at};

use indoc::indoc;
use serde_json::{Value as JsonValue, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// Both fixtures name the package after the file they came from, so an assertion on the
// name says which one the loader read.

const PHAST_TOML: &str = indoc! {r#"
    [package]
    name = "acme/from-phast"
"#};

const COMPOSER_JSON: &str = indoc! {r#"
    {
        "name": "acme/from-composer"
    }
"#};

fn client() -> HttpClient {
    HttpClient::new().unwrap()
}

/// A `GitHubHost` for `acme/widget` with both API and raw bases pointed at `server`.
fn host(server: &MockServer) -> GitHubHost {
    GitHubHost::new("acme", "widget", client(), server.uri(), server.uri())
}

/// Answer `route` on `server` with `body`, as the GitHub API would.
async fn serve(server: &MockServer, route: &str, body: JsonValue) {
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

/// Answer `route` on `server` with `code` and no body.
async fn status(server: &MockServer, route: &str, code: u16) {
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(ResponseTemplate::new(code))
        .mount(server)
        .await;
}

/// Serve `body` for `file` at `reference` on the raw-content path, which is a
/// different host from the API in production and so a separate mount here.
async fn raw(server: &MockServer, reference: &str, file: &str, body: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/acme/widget/{reference}/{file}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(server)
        .await;
}

/// Why `call` failed, or a panic if it did not.
fn refusal<T>(outcome: anyhow::Result<T>) -> String {
    match outcome {
        Ok(_) => panic!("expected the request to fail"),
        Err(error) => format!("{error:#}"),
    }
}

// ---------------------------------------------------------------------------
// Recognizing a repository URL
// ---------------------------------------------------------------------------

#[test]
fn a_github_url_resolves_with_or_without_the_git_suffix() {
    assert!(host_for("https://github.com/acme/widget", client()).is_some());
    assert!(matches!(
        host_for("https://github.com/acme/widget.git", client()),
        Some(Host::GitHub(_)),
    ));
}

#[test]
fn a_url_naming_no_repository_is_not_a_github_source() {
    // Owner but no repository, and a repository with an empty owner: neither
    // addresses anything the API could be asked about.
    assert!(GitHubHost::parse("https://github.com/acme", client()).is_none());
    assert!(GitHubHost::parse("https://github.com/acme/", client()).is_none());
    assert!(GitHubHost::parse("https://github.com//widget", client()).is_none());
}

#[test]
fn segments_past_the_repository_are_ignored() {
    // A URL copied out of the browser while looking at a branch still names the
    // repository, so it resolves rather than being rejected for the extra path.
    let host = GitHubHost::parse("https://github.com/acme/widget/tree/main", client())
        .expect("the owner and repo are both there");
    assert_eq!(
        host.dist("v1.0.0").url,
        "https://api.github.com/repos/acme/widget/zipball/v1.0.0",
    );
}

#[test]
fn a_host_with_no_driver_is_not_recognized() {
    assert!(host_for("https://gitlab.com/acme/widget", client()).is_none());
    assert!(host_for("https://example.com/acme/widget", client()).is_none());
    assert!(host_for("not a url", client()).is_none());
}

#[test]
fn an_scp_style_remote_is_not_a_url_but_an_ssh_one_is() {
    // `git@github.com:acme/widget.git` is git's own syntax, not a URL, so it
    // parses as nothing; the `ssh://` spelling does parse and names the host,
    // and the driver reaches the repository over the API either way.
    assert!(host_for("git@github.com:acme/widget.git", client()).is_none());
    assert!(host_for("ssh://git@github.com/acme/widget.git", client()).is_some());
}

// ---------------------------------------------------------------------------
// Listing the refs a repository publishes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn refs_lists_tags_then_branches() {
    let server = MockServer::start().await;
    serve(
        &server,
        "/repos/acme/widget/tags",
        json!([
            { "name": "v1.1.0", "commit": { "sha": "aaa" } },
            { "name": "v1.0.0", "commit": { "sha": "bbb" } },
        ]),
    )
    .await;
    serve(
        &server,
        "/repos/acme/widget/branches",
        json!([{ "name": "main", "commit": { "sha": "ccc" } }]),
    )
    .await;

    let refs = host(&server).refs().await.unwrap();

    let names: Vec<_> = refs.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["v1.1.0", "v1.0.0", "main"]);
    assert_eq!(refs[0].kind, RefKind::Tag);
    assert_eq!(refs[0].commit, "aaa");
    assert_eq!(refs[2].kind, RefKind::Branch);
}

#[tokio::test]
async fn a_repository_with_no_tags_or_branches_lists_nothing() {
    let server = MockServer::start().await;
    serve(&server, "/repos/acme/widget/tags", json!([])).await;
    serve(&server, "/repos/acme/widget/branches", json!([])).await;

    // An empty repository is not an error here; it is the resolver that reports
    // having no candidate to choose.
    assert!(host(&server).refs().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_listing_that_fails_names_the_endpoint_and_the_repository() {
    let server = MockServer::start().await;
    status(&server, "/repos/acme/widget/tags", 403).await;

    // A rate-limited or private repository is the common case, and the message
    // has to say which repository was refused.
    let error = refusal(host(&server).refs().await);
    assert!(error.contains("failed to list tags"), "{error}");
    assert!(error.contains("acme/widget"), "{error}");
}

#[tokio::test]
async fn a_branch_listing_failure_is_reported_separately_from_tags() {
    let server = MockServer::start().await;
    serve(&server, "/repos/acme/widget/tags", json!([])).await;
    status(&server, "/repos/acme/widget/branches", 500).await;

    // The two endpoints are fetched separately, so the half that failed is named
    // rather than the pair being reported as one failure.
    let error = refusal(host(&server).refs().await);
    assert!(error.contains("failed to list branches"), "{error}");
}

// ---------------------------------------------------------------------------
// Reading a file at a ref
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_file_that_is_not_at_the_ref_reads_as_absent() {
    let server = MockServer::start().await;
    // Nothing mounted: the raw host 404s, which is "not present at this ref".
    let bytes = host(&server).file("v1.0.0", "phast.toml").await.unwrap();
    assert!(bytes.is_none());
}

#[tokio::test]
async fn a_raw_host_that_errs_is_not_read_as_absent() {
    let server = MockServer::start().await;
    status(&server, "/acme/widget/v1.0.0/phast.toml", 500).await;

    // Only a 404 means the file is not there. Treating a 5xx the same way would
    // read a failing host as a repository that declares no manifest.
    let error = refusal(host(&server).file("v1.0.0", "phast.toml").await);
    assert!(error.contains("phast.toml"), "{error}");
    assert!(error.contains("v1.0.0"), "{error}");
}

#[tokio::test]
async fn manifest_at_prefers_composer_json() {
    let server = MockServer::start().await;
    // Both formats present at the ref; composer one wins.
    raw(&server, "v1.0.0", "composer.json", COMPOSER_JSON).await;
    raw(&server, "v1.0.0", "phast.toml", PHAST_TOML).await;

    let manifest = manifest_at(&host(&server), "v1.0.0").await.unwrap();
    assert_eq!(manifest.name.as_deref(), Some("acme/from-composer"));
}

#[tokio::test]
async fn manifest_at_falls_back_to_phast_toml() {
    let server = MockServer::start().await;
    // No composer.json (404) -> fall back to phast.toml.
    raw(&server, "v1.0.0", "phast.toml", PHAST_TOML).await;

    let manifest = manifest_at(&host(&server), "v1.0.0").await.unwrap();
    assert_eq!(manifest.name.as_deref(), Some("acme/from-phast"));
}

#[tokio::test]
async fn manifest_at_errors_when_neither_exists() {
    let server = MockServer::start().await;

    let error = refusal(manifest_at(&host(&server), "v1.0.0").await);
    assert!(error.contains("composer.json"), "{error}");
    assert!(error.contains("phast.toml"), "{error}");
}

// ---------------------------------------------------------------------------
// The default branch and when a commit landed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_default_branch_comes_from_the_repository_itself() {
    let server = MockServer::start().await;
    serve(
        &server,
        "/repos/acme/widget",
        json!({ "default_branch": "trunk" }),
    )
    .await;

    // A `[sources]` pin naming no ref resolves to whatever the repository calls
    // its default, which is not always `main`.
    assert_eq!(host(&server).branch().await.unwrap(), "trunk");
}

#[tokio::test]
async fn a_repository_that_cannot_be_read_has_no_default_branch() {
    let server = MockServer::start().await;
    status(&server, "/repos/acme/widget", 404).await;

    let error = refusal(host(&server).branch().await);
    assert!(error.contains("failed to read acme/widget"), "{error}");
}

#[tokio::test]
async fn the_commit_date_is_the_committers_not_the_authors() {
    let server = MockServer::start().await;
    serve(
        &server,
        "/repos/acme/widget/commits/abc123",
        json!({ "commit": {
            "author":    { "date": "2020-01-01T00:00:00Z" },
            "committer": { "date": "2026-03-04T05:06:07Z" },
        }}),
    )
    .await;

    // A rebased or cherry-picked patch keeps its original author date, which can
    // predate the release by years; the committer date is when it landed. The
    // `Z` becomes `+00:00`, the form a registry-sourced entry records.
    let date = host(&server).committed_at("abc123").await.unwrap();
    assert_eq!(date.as_deref(), Some("2026-03-04T05:06:07+00:00"));
}

#[tokio::test]
async fn a_commit_date_that_does_not_parse_is_dropped() {
    let server = MockServer::start().await;
    serve(
        &server,
        "/repos/acme/widget/commits/abc123",
        json!({ "commit": { "committer": { "date": "the fourth of March" } } }),
    )
    .await;

    // The lock records the date for information, so an unreadable one leaves the
    // field out rather than failing the lock over it.
    let date = host(&server).committed_at("abc123").await.unwrap();
    assert!(date.is_none());
}

#[tokio::test]
async fn a_commit_that_is_not_there_is_reported() {
    let server = MockServer::start().await;
    status(&server, "/repos/acme/widget/commits/nope", 404).await;

    let error = refusal(host(&server).committed_at("nope").await);
    assert!(error.contains("failed to read commit nope"), "{error}");
    assert!(error.contains("acme/widget"), "{error}");
}

// ---------------------------------------------------------------------------
// The archive to download
// ---------------------------------------------------------------------------

#[tokio::test]
async fn dist_points_at_the_zipball() {
    let server = MockServer::start().await;
    let dist = host(&server).dist("v1.0.0");

    // GitHub publishes no checksum for a zipball, so the archive is accepted on
    // the reference alone.
    assert_eq!(
        dist.url,
        format!("{}/repos/acme/widget/zipball/v1.0.0", server.uri()),
    );
    assert_eq!(dist.reference.as_deref(), Some("v1.0.0"));
    assert!(dist.shasum.is_none());
}

#[tokio::test]
async fn dist_is_built_without_asking_the_host() {
    let server = MockServer::start().await;
    let dist = host(&server).dist("abc123");

    // Nothing is mounted and the URL still comes back: the zipball address is
    // derived from the reference, so building a lock entry costs no request.
    assert!(dist.url.ends_with("/zipball/abc123"));
    assert!(server.received_requests().await.unwrap().is_empty());
}
