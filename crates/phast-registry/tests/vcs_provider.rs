use std::str::FromStr;

use phast_core::manifest::GitRef;
use phast_core::package::PackageName;
use phast_registry::http::client::HttpClient;
use phast_registry::provider::{Lockable, VcsProvider};
use phast_registry::router::RegistryOptions;
use phast_registry::vcs::Host;
use phast_registry::vcs::github::GitHubHost;
use phast_resolver::provider::Provider;
use phast_version::parser::parse_version;
use phast_version::{Constraint, Stability, Version};

use indoc::indoc;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The repository's manifest: the name the provider discovers, and one
/// requirement to carry through metadata and into the lock.
const MANIFEST: &str = indoc! {r#"
    {
        "name": "acme/widget",
        "require": { "psr/log": "^3.0" }
    }
"#};

/// The same manifest, with `main` presenting as a numeric dev version.
const ALIASED: &str = indoc! {r#"
    {
        "name": "acme/widget",
        "require": { "psr/log": "^3.0" },
        "extra": { "branch-alias": { "dev-main": "1.2.x-dev" } }
    }
"#};

/// The same manifest declaring a version of its own, which is all a pin at a bare
/// commit has to go on.
const VERSIONED: &str = indoc! {r#"
    {
        "name": "acme/widget",
        "version": "2.5.0",
        "require": { "psr/log": "^3.0" }
    }
"#};

/// A host addressing `acme/widget` with both API and raw bases on `server`.
fn host(server: &MockServer) -> Host {
    Host::GitHub(GitHubHost::new(
        "acme",
        "widget",
        HttpClient::new().unwrap(),
        server.uri(),
        server.uri(),
    ))
}

/// Mount `acme/widget` on `server`: the tag `v1.0.0` at commit `c1`, the branch
/// `main` at `c2` and named as the default, and `manifest` readable at every ref
/// a test reaches for. No commit date is served, so a lock records none unless
/// the test asks for one.
async fn repository(server: &MockServer, manifest: &str) {
    Mock::given(method("GET"))
        .and(path("/repos/acme/widget/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "name": "v1.0.0", "commit": { "sha": "c1" } },
        ])))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widget/branches"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "name": "main", "commit": { "sha": "c2" } },
        ])))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widget"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "default_branch": "main",
        })))
        .mount(server)
        .await;

    for reference in ["HEAD", "c1", "c2", "deadbeef"] {
        Mock::given(method("GET"))
            .and(path(format!("/acme/widget/{reference}/composer.json")))
            .respond_with(ResponseTemplate::new(200).set_body_string(manifest))
            .mount(server)
            .await;
    }
}

/// Report `date` as when `commit` landed, which is what the lock's `time` reads.
async fn committed(server: &MockServer, commit: &str, date: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/repos/acme/widget/commits/{commit}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "commit": { "committer": { "date": date } },
        })))
        .mount(server)
        .await;
}

/// A provider that discovered the repository, offering every ref it lists.
/// `minimum_stability` gates the branch aliases and nothing else.
async fn discovered(
    server: &MockServer,
    manifest: &str,
    minimum_stability: Stability,
) -> VcsProvider {
    repository(server, manifest).await;
    VcsProvider::discover(
        "https://github.com/acme/widget",
        host(server),
        RegistryOptions {
            minimum_stability,
            minimum_package_age: None,
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

/// A provider pinned to `reference`, offering exactly the one version it names.
async fn pinned(server: &MockServer, manifest: &str, reference: GitRef) -> VcsProvider {
    repository(server, manifest).await;
    VcsProvider::pin(
        "https://github.com/acme/widget",
        host(server),
        &reference,
        &PackageName::new("acme/widget"),
    )
    .await
    .unwrap()
}

/// Why the call failed, or a panic if it did not.
fn refusal<T>(outcome: anyhow::Result<T>) -> String {
    match outcome {
        Ok(_) => panic!("expected the call to fail"),
        Err(error) => format!("{error:#}"),
    }
}

// ---------------------------------------------------------------------------
// Discovering every ref a repository publishes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_name_comes_from_the_manifest_and_every_ref_becomes_a_version() {
    let server = MockServer::start().await;
    let provider = discovered(&server, MANIFEST, Stability::Stable).await;
    let name = provider.name().clone();
    assert_eq!(name.as_str(), "acme/widget");

    // A tag surfaces as its number and a branch as `dev-<branch>`, so a require
    // naming either shape has something to select.
    let versions = provider.versions(&name).await.unwrap();
    assert_eq!(versions.len(), 2);
    assert!(versions.contains(&parse_version("1.0.0").unwrap()));
    assert!(versions.contains(&Version::Branch("main".to_owned())));
}

#[tokio::test]
async fn a_repository_whose_manifest_declares_no_name_is_refused() {
    let server = MockServer::start().await;
    repository(&server, r#"{ "require": { "psr/log": "^3.0" } }"#).await;

    // Nothing in a `vcs` repository entry says which package it holds, so the
    // manifest is the only thing that can, and there is no pinning it without.
    let outcome = VcsProvider::discover(
        "https://github.com/acme/widget",
        host(&server),
        RegistryOptions::default(),
    )
    .await;
    assert!(refusal(outcome).contains("declares no package name"));
}

#[tokio::test]
async fn a_repository_with_no_tags_or_branches_offers_no_versions() {
    let server = MockServer::start().await;
    for endpoint in ["tags", "branches"] {
        Mock::given(method("GET"))
            .and(path(format!("/repos/acme/widget/{endpoint}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/acme/widget/HEAD/composer.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(MANIFEST))
        .mount(&server)
        .await;

    // An empty repository still names a package, so discovery succeeds and it is
    // resolution that reports having no candidate.
    let provider = VcsProvider::discover(
        "https://github.com/acme/widget",
        host(&server),
        RegistryOptions::default(),
    )
    .await
    .unwrap();
    assert!(provider.versions(provider.name()).await.unwrap().is_empty());
}

#[tokio::test]
async fn versions_are_empty_for_a_name_the_provider_does_not_serve() {
    let server = MockServer::start().await;
    let provider = discovered(&server, MANIFEST, Stability::Stable).await;
    let other = PackageName::new("someone/else");

    assert!(provider.versions(&other).await.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Offering a branch under its numeric alias
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_branch_alias_is_offered_when_dev_is_permitted() {
    let server = MockServer::start().await;
    let provider = discovered(&server, ALIASED, Stability::Dev).await;
    let name = provider.name().clone();

    // Tag, branch, and the 1.2.x-dev the branch presents as.
    let versions = provider.versions(&name).await.unwrap();
    assert_eq!(versions.len(), 3);
    assert!(versions.contains(&Version::Branch("main".to_owned())));

    // The aliased version is what lets a numeric ^1.2 require select the branch.
    let caret = Constraint::from_str("^1.2").unwrap();
    assert!(
        versions.iter().any(|version| caret.matches(version)),
        "an aliased version should satisfy ^1.2: {versions:?}",
    );
}

#[tokio::test]
async fn a_branch_alias_is_gated_out_under_a_stable_floor() {
    let server = MockServer::start().await;
    let provider = discovered(&server, ALIASED, Stability::Stable).await;
    let name = provider.name().clone();

    // The floor gates the aliases and nothing else: the tag and the branch are
    // both still offered, so a require naming `dev-main` still resolves.
    let versions = provider.versions(&name).await.unwrap();
    assert_eq!(versions.len(), 2);
    let caret = Constraint::from_str("^1.2").unwrap();
    assert!(
        !versions.iter().any(|version| caret.matches(version)),
        "no version should satisfy ^1.2 under stable: {versions:?}",
    );
}

#[tokio::test]
async fn a_branch_alias_locks_as_the_branch_at_its_commit() {
    let server = MockServer::start().await;
    let provider = discovered(&server, ALIASED, Stability::Dev).await;
    let name = provider.name().clone();

    // Resolve the alias (the only version satisfying ^1.2) and lock it.
    let caret = Constraint::from_str("^1.2").unwrap();
    let alias = provider
        .versions(&name)
        .await
        .unwrap()
        .into_iter()
        .find(|version| caret.matches(version))
        .expect("an aliased version satisfies ^1.2");

    let locked = provider.lock(provider.name(), &alias).await.unwrap();
    // Locks as the branch itself, not the alias's sentinel-expanded numeric...
    assert_eq!(locked.version, "dev-main");
    // ...pinned at the branch commit.
    assert_eq!(locked.source.unwrap().reference, "c2");
}

// ---------------------------------------------------------------------------
// Pinning exactly one ref
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_branch_pin_offers_only_that_branch() {
    let server = MockServer::start().await;
    let provider = pinned(&server, MANIFEST, GitRef::Branch("main".to_owned())).await;

    // A pin is the choice already made, so the tag the repository also carries is
    // not offered: exposing every ref would make a pin and a discovery identical.
    let versions = provider.versions(provider.name()).await.unwrap();
    assert_eq!(versions, vec![Version::Branch("main".to_owned())]);
}

#[tokio::test]
async fn a_tag_pin_offers_only_that_tag() {
    let server = MockServer::start().await;
    let provider = pinned(&server, MANIFEST, GitRef::Tag("v1.0.0".to_owned())).await;

    let versions = provider.versions(provider.name()).await.unwrap();
    assert_eq!(versions, vec![parse_version("1.0.0").unwrap()]);
}

#[tokio::test]
async fn a_ref_less_pin_uses_the_default_branch() {
    let server = MockServer::start().await;
    let provider = pinned(&server, MANIFEST, GitRef::Default).await;

    let versions = provider.versions(provider.name()).await.unwrap();
    assert_eq!(versions, vec![Version::Branch("main".to_owned())]);
}

#[tokio::test]
async fn a_commit_pin_takes_its_version_from_the_manifest() {
    let server = MockServer::start().await;
    let provider = pinned(&server, VERSIONED, GitRef::Reference("deadbeef".to_owned())).await;

    // The commit names no version of its own, so the manifest at it supplies one.
    let versions = provider.versions(provider.name()).await.unwrap();
    assert_eq!(versions, vec![parse_version("2.5.0").unwrap()]);
}

#[tokio::test]
async fn a_commit_pin_asks_for_nothing_but_the_manifest() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/acme/widget/deadbeef/composer.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(VERSIONED))
        .mount(&server)
        .await;

    // No tags, branches or repository endpoint is mounted and the pin still
    // resolves: a commit is already a commit, so there is no listing to search.
    VcsProvider::pin(
        "https://github.com/acme/widget",
        host(&server),
        &GitRef::Reference("deadbeef".to_owned()),
        &PackageName::new("acme/widget"),
    )
    .await
    .unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_commit_pin_whose_manifest_declares_no_version_is_refused() {
    let server = MockServer::start().await;
    repository(&server, MANIFEST).await;

    // A tag or branch carries a version in its name; a bare commit does not, so
    // a manifest without one leaves nothing to resolve the pin against.
    let outcome = VcsProvider::pin(
        "https://github.com/acme/widget",
        host(&server),
        &GitRef::Reference("deadbeef".to_owned()),
        &PackageName::new("acme/widget"),
    )
    .await;

    let error = refusal(outcome);
    assert!(error.contains("pinned to commit deadbeef"), "{error}");
    assert!(error.contains("declares no version"), "{error}");
}

#[tokio::test]
async fn a_commit_pin_whose_version_does_not_parse_is_refused() {
    let server = MockServer::start().await;
    repository(
        &server,
        r#"{ "name": "acme/widget", "version": "not a version" }"#,
    )
    .await;

    let outcome = VcsProvider::pin(
        "https://github.com/acme/widget",
        host(&server),
        &GitRef::Reference("deadbeef".to_owned()),
        &PackageName::new("acme/widget"),
    )
    .await;

    let error = refusal(outcome);
    assert!(error.contains("invalid version 'not a version'"), "{error}");
    assert!(error.contains("acme/widget"), "{error}");
}

#[tokio::test]
async fn a_pin_locks_the_commit_its_ref_resolves_to() {
    let server = MockServer::start().await;
    let provider = pinned(&server, MANIFEST, GitRef::Tag("v1.0.0".to_owned())).await;

    let locked = provider
        .lock(provider.name(), &parse_version("1.0.0").unwrap())
        .await
        .unwrap();
    // The tag resolves to its commit, so the lock pins the commit, not the name.
    assert_eq!(locked.source.unwrap().reference, "c1");
    // `1.0`, not `1.0.0`: version rendering drops trailing zero segments.
    assert_eq!(locked.version, "1.0");
}

#[tokio::test]
async fn a_branch_pin_locks_as_a_dev_version_at_the_branch_commit() {
    let server = MockServer::start().await;
    let provider = pinned(&server, MANIFEST, GitRef::Branch("main".to_owned())).await;

    let locked = provider
        .lock(provider.name(), &Version::Branch("main".to_owned()))
        .await
        .unwrap();
    assert_eq!(locked.version, "dev-main");
    assert_eq!(locked.source.unwrap().reference, "c2");
}

#[tokio::test]
async fn a_pin_rejects_a_repository_declaring_another_name() {
    let server = MockServer::start().await;
    repository(&server, MANIFEST).await;

    // The name comes from the [sources] key rather than the manifest here, so a
    // disagreement means the pin would install something else entirely.
    let outcome = VcsProvider::pin(
        "https://github.com/acme/widget",
        host(&server),
        &GitRef::Branch("main".to_owned()),
        &PackageName::new("acme/something-else"),
    )
    .await;

    let error = refusal(outcome);
    assert!(error.contains("acme/something-else"), "{error}");
    assert!(error.contains("acme/widget"), "{error}");
}

#[tokio::test]
async fn a_pin_at_an_unknown_tag_fails_naming_it() {
    let server = MockServer::start().await;
    repository(&server, MANIFEST).await;

    let outcome = VcsProvider::pin(
        "https://github.com/acme/widget",
        host(&server),
        &GitRef::Tag("v9.9.9".to_owned()),
        &PackageName::new("acme/widget"),
    )
    .await;

    assert!(refusal(outcome).contains("no tag named 'v9.9.9'"));
}

#[tokio::test]
async fn a_pin_at_an_unknown_branch_says_branch_rather_than_tag() {
    let server = MockServer::start().await;
    repository(&server, MANIFEST).await;

    // The message names the shape that was looked for, so a typo in a branch pin
    // does not send the reader looking through the tag list.
    let outcome = VcsProvider::pin(
        "https://github.com/acme/widget",
        host(&server),
        &GitRef::Branch("mian".to_owned()),
        &PackageName::new("acme/widget"),
    )
    .await;

    assert!(refusal(outcome).contains("no branch named 'mian'"));
}

// ---------------------------------------------------------------------------
// What the resolver is told
// ---------------------------------------------------------------------------

#[tokio::test]
async fn metadata_reads_the_manifest_at_the_resolved_ref() {
    let server = MockServer::start().await;
    let provider = discovered(&server, MANIFEST, Stability::Stable).await;
    let name = provider.name().clone();

    let meta = provider
        .metadata(&name, &parse_version("1.0.0").unwrap())
        .await
        .unwrap();
    assert_eq!(meta.require.len(), 1);
    assert_eq!(meta.require[0].0.as_str(), "psr/log");
}

#[tokio::test]
async fn replace_and_provide_specs_resolve_against_the_resolved_version() {
    let server = MockServer::start().await;
    let manifest = indoc! {r#"
        {
            "name": "acme/widget",
            "replace": { "acme/old": "self.version" },
            "provide": { "psr/log-implementation": "*" }
        }
    "#};
    let provider = discovered(&server, manifest, Stability::Stable).await;
    let name = provider.name().clone();
    let version = parse_version("1.0.0").unwrap();

    // Both sentinels resolve to the version the ref surfaced as, so a git package
    // states what it replaces and provides exactly as a registry one would.
    let meta = provider.metadata(&name, &version).await.unwrap();
    assert_eq!(
        meta.replace,
        vec![(PackageName::new("acme/old"), version.clone())],
    );
    assert_eq!(
        meta.provide,
        vec![(PackageName::new("psr/log-implementation"), version)],
    );
}

#[tokio::test]
async fn dev_requirements_are_never_offered() {
    let server = MockServer::start().await;
    let manifest = indoc! {r#"
        {
            "name": "acme/widget",
            "require-dev": { "phpunit/phpunit": "^10.0" }
        }
    "#};
    let provider = discovered(&server, manifest, Stability::Stable).await;

    // An installed dependency's own dev requirements are not resolved, whichever
    // kind of source supplied it.
    let meta = provider
        .metadata(provider.name(), &parse_version("1.0.0").unwrap())
        .await
        .unwrap();
    assert!(meta.require_dev.is_empty());
}

#[tokio::test]
async fn a_version_no_ref_surfaced_is_an_error() {
    let server = MockServer::start().await;
    let provider = discovered(&server, MANIFEST, Stability::Stable).await;
    let name = provider.name().clone();
    let absent = parse_version("9.9.9").unwrap();

    // Both entry points look the version up among the candidates computed at
    // construction, so neither goes to the host for a ref that was never offered.
    assert!(
        refusal(provider.metadata(&name, &absent).await).contains("no ref of acme/widget matches"),
    );
    assert!(refusal(provider.lock(&name, &absent).await).contains("no ref of acme/widget matches"));
}

// ---------------------------------------------------------------------------
// The lock entry
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_lock_entry_records_the_zipball_and_the_git_source() {
    let server = MockServer::start().await;
    let provider = discovered(&server, MANIFEST, Stability::Stable).await;

    let locked = provider
        .lock(provider.name(), &parse_version("1.0.0").unwrap())
        .await
        .unwrap();
    assert_eq!(locked.name, "acme/widget");

    // The archive is the zipball at the resolved commit, and the source pins that
    // same commit, so neither moves when the tag does.
    let dist = locked.dist.expect("vcs package carries a dist");
    assert!(dist.url.ends_with("/repos/acme/widget/zipball/c1"));

    let source = locked.source.expect("vcs package carries a source");
    assert_eq!(source.reference, "c1");
    assert_eq!(source.url, "https://github.com/acme/widget");

    assert!(locked.require.entries.contains_key("psr/log"));
    assert!(locked.path.is_none(), "a git package is not a member");
}

#[tokio::test]
async fn the_manifest_fields_a_consumer_reads_are_carried_over() {
    let server = MockServer::start().await;
    let manifest = indoc! {r#"
        {
            "name": "acme/widget",
            "type": "library",
            "description": "A widget",
            "license": "MIT",
            "autoload": {
                "psr-4": {
                    "Acme\\Widget\\": "src/"
                }
            }
        }
    "#};
    let provider = discovered(&server, manifest, Stability::Stable).await;

    // Read from the manifest at the resolved ref rather than at HEAD, so a lock
    // describes the release it pins and not whatever the branch says today.
    let locked = provider
        .lock(provider.name(), &parse_version("1.0.0").unwrap())
        .await
        .unwrap();
    assert_eq!(locked.package_type.as_deref(), Some("library"));
    assert_eq!(locked.description.as_deref(), Some("A widget"));
    assert_eq!(locked.license, ["MIT"]);
    assert!(locked.autoload.psr4.contains_key("Acme\\Widget\\"));
}

#[tokio::test]
async fn the_lock_carries_the_manifest_extra_block() {
    let server = MockServer::start().await;
    let provider = discovered(&server, ALIASED, Stability::Dev).await;

    // The `main` branch, whose manifest declares extra.branch-alias.
    let locked = provider
        .lock(provider.name(), &Version::Branch("main".to_owned()))
        .await
        .unwrap();

    let extra = locked.extra.expect("the repo manifest declares extra").data;
    assert_eq!(extra["branch-alias"]["dev-main"], "1.2.x-dev");
}

#[tokio::test]
async fn the_commit_date_stands_in_for_a_publish_date() {
    let server = MockServer::start().await;
    let provider = discovered(&server, MANIFEST, Stability::Stable).await;
    committed(&server, "c1", "2024-09-11T13:17:53Z").await;

    let locked = provider
        .lock(provider.name(), &parse_version("1.0.0").unwrap())
        .await
        .unwrap();

    // A repository manifest has no publish date, so the host's commit date stands in,
    // normalized from GitHub's `Z` to the offset form registry metadata uses.
    assert_eq!(locked.time.as_deref(), Some("2024-09-11T13:17:53+00:00"));
}

#[tokio::test]
async fn a_missing_commit_date_does_not_fail_the_lock() {
    let server = MockServer::start().await;
    // No commits endpoint is mounted, so the date lookup fails.
    let provider = discovered(&server, MANIFEST, Stability::Stable).await;

    let locked = provider
        .lock(provider.name(), &parse_version("1.0.0").unwrap())
        .await
        .expect("a missing timestamp must not fail the lock");

    assert_eq!(locked.time, None);
}

#[tokio::test]
async fn locking_another_name_is_an_error() {
    let server = MockServer::start().await;
    let provider = discovered(&server, MANIFEST, Stability::Stable).await;
    let other = PackageName::new("someone/else");
    let version = parse_version("1.0.0").unwrap();

    let error = refusal(provider.lock(&other, &version).await);
    assert!(error.contains("acme/widget"), "got: {error}");
    assert!(error.contains("someone/else"), "got: {error}");
}
