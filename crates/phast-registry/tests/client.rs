use phast_registry::http::client::HttpClient;

/// A client with `secure-http` enforcement on, as `new` leaves it.
fn client() -> HttpClient {
    HttpClient::new().unwrap()
}

/// Whether the guard lets a GET for `url` be built.
fn allows(url: &str) -> bool {
    client().get(url).is_ok()
}

/// Why the guard refused `url`, or a panic if it did not.
fn refusal(url: &str) -> String {
    match client().get(url) {
        Ok(_) => panic!("expected {url} to be refused"),
        Err(error) => error.to_string(),
    }
}

// ---------------------------------------------------------------------------
// What gets through
// ---------------------------------------------------------------------------

#[test]
fn allows_https() {
    assert!(allows("https://packagist.org/packages.json"));
    assert!(client().post("https://packagist.org/api").is_ok());
}

#[test]
fn allows_plaintext_http_to_loopback() {
    // wiremock and other local test servers bind to loopback over http.
    assert!(allows("http://127.0.0.1:8080/packages.json"));
    assert!(allows("http://localhost:8080/packages.json"));
    assert!(allows("http://[::1]:8080/packages.json"));
}

#[test]
fn the_whole_127_range_is_loopback() {
    // `127.0.0.0/8`, not just `127.0.0.1`, so a server bound anywhere in the
    // range is reachable over plaintext.
    assert!(allows("http://127.1.2.3:8080/packages.json"));
}

#[test]
fn a_localhost_subdomain_is_loopback_but_a_lookalike_is_not() {
    // `*.localhost` is reserved for the local machine, so it is exempt. The
    // check is anchored on the suffix, so a domain merely starting with the
    // word is a remote host like any other.
    assert!(allows("http://app.localhost:8080/packages.json"));
    assert!(!allows("http://localhost.evil.example/packages.json"));
    assert!(!allows("http://127.0.0.1.evil.example/packages.json"));
}

// ---------------------------------------------------------------------------
// What is refused
// ---------------------------------------------------------------------------

#[test]
fn rejects_plaintext_http_to_remote_host() {
    let error = refusal("http://packagist.org/packages.json");
    assert!(error.contains("insecure HTTP"), "unexpected error: {error}");
}

#[test]
fn post_is_guarded_like_get() {
    let error = client()
        .post("http://packagist.org/api")
        .unwrap_err()
        .to_string();
    assert!(error.contains("insecure HTTP"), "unexpected error: {error}");
}

#[test]
fn a_scheme_that_is_not_https_is_refused() {
    // An allowlist rather than a ban on `http://`: nothing but https and
    // loopback gets through, whatever the scheme.
    refusal("ftp://packagist.org/packages.json");
    refusal("file:///etc/passwd");
}

#[test]
fn a_malformed_url_is_refused() {
    // The guard cannot tell what it would be talking to, so it refuses rather
    // than handing the string to the HTTP stack to interpret.
    let error = refusal("not a url at all");
    assert!(error.contains("invalid URL"), "unexpected error: {error}");
}

// ---------------------------------------------------------------------------
// Turning the guard off
// ---------------------------------------------------------------------------

#[test]
fn opting_out_allows_plaintext_http() {
    let client = client().with_secure_http(false);
    assert!(client.get("http://packagist.org/packages.json").is_ok());
}

#[test]
fn opting_out_stops_the_url_being_checked_here() {
    // The guard returns before parsing, so turning it off gives up the URL check
    // along with the scheme check. The request is not valid, it just fails later,
    // when it is sent, rather than while it is being built.
    let client = client().with_secure_http(false);
    assert!(client.get("not a url at all").is_ok());
}
