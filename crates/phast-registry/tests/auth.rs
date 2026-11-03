use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use phast_registry::http::auth::{AuthConfig, BasicAuth, Credential};

use indoc::indoc;

/// A one-entry token table for `host`, as the bearer and OAuth fields hold them.
fn tokens(host: &str, token: &str) -> HashMap<String, String> {
    HashMap::from([(host.to_owned(), token.to_owned())])
}

/// A one-entry basic-auth table for `host`.
fn basic(host: &str, username: &str, password: &str) -> HashMap<String, BasicAuth> {
    HashMap::from([(
        host.to_owned(),
        BasicAuth {
            username: username.to_owned(),
            password: password.to_owned(),
        },
    )])
}

/// The credential `auth` offers for `url_or_host`, rendered so a failing test
/// reports what it found rather than only that a match failed.
fn credential(auth: &AuthConfig, url_or_host: &str) -> String {
    match auth.credential_for(url_or_host) {
        Some(Credential::Bearer(token)) => format!("bearer {token}"),
        Some(Credential::Basic { username, password }) => format!("basic {username}:{password}"),
        None => "none".to_owned(),
    }
}

/// A file holding `contents`, kept alive by the returned handle.
fn file(contents: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    write!(file, "{contents}").unwrap();
    file
}

// ---------------------------------------------------------------------------
// Which credential a host is offered
// ---------------------------------------------------------------------------

#[test]
fn returns_none_for_unknown_host() {
    let auth = AuthConfig::default();
    assert_eq!(credential(&auth, "unknown.example.com"), "none");
}

#[test]
fn returns_bearer_token() {
    let auth = AuthConfig {
        bearer: tokens("example.com", "tok123"),
        ..Default::default()
    };
    assert_eq!(credential(&auth, "example.com"), "bearer tok123");
}

#[test]
fn returns_github_oauth_as_bearer() {
    let auth = AuthConfig {
        github_token: tokens("github.com", "ghp_abc"),
        ..Default::default()
    };
    assert_eq!(credential(&auth, "github.com"), "bearer ghp_abc");
}

#[test]
fn returns_gitlab_token_as_bearer() {
    let auth = AuthConfig {
        gitlab_token: tokens("gitlab.com", "glpat_xyz"),
        ..Default::default()
    };
    assert_eq!(credential(&auth, "gitlab.com"), "bearer glpat_xyz");
}

#[test]
fn returns_basic_auth() {
    let auth = AuthConfig {
        http_basic: basic("repo.example.com", "user", "pass"),
        ..Default::default()
    };
    assert_eq!(credential(&auth, "repo.example.com"), "basic user:pass");
}

#[test]
fn a_full_url_is_reduced_to_its_host() {
    let auth = AuthConfig {
        http_basic: basic("repo.example.com", "user", "pass"),
        ..Default::default()
    };

    // Callers pass whatever they have: a registry URL from the manifest, or a
    // bare hostname. A port is not part of the key either.
    assert_eq!(
        credential(&auth, "https://repo.example.com/packages.json"),
        "basic user:pass",
    );
    assert_eq!(
        credential(&auth, "https://repo.example.com:8443/packages.json"),
        "basic user:pass",
    );
}

#[test]
fn github_oauth_applies_to_github_api_subdomains() {
    // The token is keyed by github.com but the driver's requests hit these hosts.
    let auth = AuthConfig {
        github_token: tokens("github.com", "ghp_abc"),
        ..Default::default()
    };
    for url in [
        "https://api.github.com/repos/acme/widget/tags",
        "https://raw.githubusercontent.com/acme/widget/main/composer.json",
        "https://codeload.github.com/acme/widget/zip/refs/tags/v1.0.0",
    ] {
        assert_eq!(credential(&auth, url), "bearer ghp_abc", "for {url}");
    }
}

#[test]
fn github_oauth_does_not_leak_to_unrelated_hosts() {
    let auth = AuthConfig {
        github_token: tokens("github.com", "ghp_abc"),
        ..Default::default()
    };
    assert_eq!(credential(&auth, "https://gitlab.com/acme/widget"), "none");
    assert_eq!(credential(&auth, "https://example.com"), "none");
}

#[test]
fn bearer_takes_priority_over_github_oauth() {
    let auth = AuthConfig {
        bearer: tokens("example.com", "bearer_tok"),
        github_token: tokens("example.com", "github_tok"),
        ..Default::default()
    };
    assert_eq!(credential(&auth, "example.com"), "bearer bearer_tok");
}

#[test]
fn github_oauth_takes_priority_over_gitlab() {
    let auth = AuthConfig {
        github_token: tokens("example.com", "github_tok"),
        gitlab_token: tokens("example.com", "gitlab_tok"),
        ..Default::default()
    };
    assert_eq!(credential(&auth, "example.com"), "bearer github_tok");
}

#[test]
fn gitlab_takes_priority_over_basic_auth() {
    let auth = AuthConfig {
        gitlab_token: tokens("example.com", "gitlab_tok"),
        http_basic: basic("example.com", "user", "pass"),
        ..Default::default()
    };
    assert_eq!(credential(&auth, "example.com"), "bearer gitlab_tok");
}

// ---------------------------------------------------------------------------
// Layering one source of credentials under another
// ---------------------------------------------------------------------------

#[test]
fn merge_first_inserted_wins() {
    let mut local = AuthConfig {
        bearer: tokens("example.com", "token-a"),
        http_basic: basic("repo.example.com", "user-a", "pass-a"),
        ..Default::default()
    };

    local.merge(AuthConfig {
        bearer: tokens("example.com", "token-b"),
        http_basic: basic("repo.example.com", "user-b", "pass-b"),
        ..Default::default()
    });

    // The higher-priority source is merged into, so its values survive.
    assert_eq!(credential(&local, "example.com"), "bearer token-a");
    assert_eq!(
        credential(&local, "repo.example.com"),
        "basic user-a:pass-a",
    );
}

#[test]
fn merge_covers_the_oauth_tables_too() {
    let mut local = AuthConfig {
        github_token: tokens("github.com", "ghp_local"),
        ..Default::default()
    };

    let mut global = AuthConfig {
        github_token: tokens("github.com", "ghp_global"),
        gitlab_token: tokens("gitlab.com", "glpat_global"),
        ..Default::default()
    };
    global
        .github_token
        .insert("ghe.acme.example".to_owned(), "ghp_enterprise".to_owned());

    local.merge(global);

    // Every table is layered the same way: a host already known keeps its value,
    // and a host only the lower source names is picked up.
    assert_eq!(credential(&local, "github.com"), "bearer ghp_local");
    assert_eq!(
        credential(&local, "ghe.acme.example"),
        "bearer ghp_enterprise",
    );
    assert_eq!(credential(&local, "gitlab.com"), "bearer glpat_global");
}

// ---------------------------------------------------------------------------
// Reading an auth file
// ---------------------------------------------------------------------------

#[test]
fn toml_reads_the_dotted_key_format() {
    let auth_toml = file(indoc! {r#"
        [http-basic."nova.laravel.com"]
        username = "nova-user"
        password = "nova-pass"
    "#});

    let auth = AuthConfig::load_from_toml_file(auth_toml.path()).unwrap();
    assert_eq!(
        credential(&auth, "nova.laravel.com"),
        "basic nova-user:nova-pass",
    );
}

#[test]
fn json_reads_the_http_basic_section() {
    let auth_json = file(indoc! {r#"
        {
            "http-basic": {
                "host.example.com": {
                    "username": "u",
                    "password": "p"
                }
            }
        }
    "#});

    let auth = AuthConfig::load_from_json_file(auth_json.path()).unwrap();
    assert_eq!(credential(&auth, "host.example.com"), "basic u:p");
}

#[test]
fn an_absent_file_reads_as_no_credentials() {
    let missing = Path::new("/nonexistent/phast/auth.toml");

    // Not having an auth file is the ordinary case, so it cannot be an error.
    assert!(AuthConfig::load_from_toml_file(missing).unwrap().is_empty());
    assert!(
        AuthConfig::load_from_json_file(Path::new("/nonexistent/phast/auth.json"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_file_that_does_not_parse_is_an_error() {
    let broken = file("this is not toml {{");

    // Distinct from an absent file on purpose: a malformed one would otherwise
    // look like no credentials, and the request would fail as unauthorized.
    let err = AuthConfig::load_from_toml_file(broken.path()).unwrap_err();
    assert!(err.to_string().contains("failed to parse"), "{err}");
}
