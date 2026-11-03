use phast_core::manifest::{GitRef, Manifest, Source};
use phast_core::package::PackageName;

/// Parse `sources` as the `[sources]` body of an otherwise minimal manifest.
fn sources(body: &str) -> Result<Manifest, String> {
    let toml = format!("[sources]\n{body}\n");
    Manifest::from_phast_toml(&toml).map_err(|e| e.to_string())
}

/// The single source declared by `body`, or a parse error.
fn source(body: &str) -> Source {
    let manifest = sources(body).expect("manifest parses");
    manifest
        .sources
        .get(&PackageName::new("acme/lib"))
        .expect("acme/lib is declared")
        .clone()
}

// ---------------------------------------------------------------------------
// Shape classification
// ---------------------------------------------------------------------------

#[test]
fn repository_names_a_declared_endpoint() {
    assert!(matches!(
        source(r#""acme/lib" = { repository = "internal" }"#),
        Source::Repository { repository } if repository == "internal",
    ));
}

#[test]
fn path_is_a_local_package() {
    assert!(matches!(
        source(r#""acme/lib" = { path = "packages/lib" }"#),
        Source::Path { path, version } if path == "packages/lib" && version.is_none(),
    ));
}

#[test]
fn path_carries_a_declared_version() {
    assert!(matches!(
        source(r#""acme/lib" = { path = "packages/lib", version = "3.7.x-dev" }"#),
        Source::Path { version: Some(version), .. } if version == "3.7.x-dev",
    ));
}

#[test]
fn a_url_with_a_version_is_an_inline_package() {
    // `version` is what separates the two url shapes: declaring one makes the spec
    // self-describing, so the package is never fetched for metadata.
    let source = source(
        r#""acme/lib" = { url = "https://github.com/acme/lib", version = "1.4.0", reference = "abc1234" }"#,
    );
    let Source::Package { package } = source else {
        panic!("a url with a version is an inline package, got: {source:?}");
    };
    assert_eq!(package["name"], "acme/lib");
    assert_eq!(package["version"], "1.4.0");
    assert_eq!(package["source"]["reference"], "abc1234");
}

#[test]
fn a_bare_url_is_a_git_source() {
    assert!(matches!(
        source(r#""acme/lib" = { url = "https://github.com/acme/lib" }"#),
        Source::Git { reference: GitRef::Default, url } if url == "https://github.com/acme/lib",
    ));
}

#[test]
fn a_git_source_takes_the_ref_it_names() {
    assert!(matches!(
        source(r#""acme/lib" = { url = "https://x/y", branch = "main" }"#),
        Source::Git { reference: GitRef::Branch(branch), .. } if branch == "main",
    ));
    assert!(matches!(
        source(r#""acme/lib" = { url = "https://x/y", tag = "v2.3.1" }"#),
        Source::Git { reference: GitRef::Tag(tag), .. } if tag == "v2.3.1",
    ));
    assert!(matches!(
        source(r#""acme/lib" = { url = "https://x/y", reference = "abc1234" }"#),
        Source::Git { reference: GitRef::Reference(commit), .. } if commit == "abc1234",
    ));
}

#[test]
fn an_inline_package_records_a_null_reference_when_none_is_given() {
    let source = source(r#""acme/lib" = { url = "https://x/y", version = "1.4.0" }"#);
    let Source::Package { package } = source else {
        panic!("a url with a version is an inline package, got: {source:?}");
    };

    // The key is always written, so a consumer reads a null rather than having
    // to tell a missing reference from an absent `source` block.
    assert_eq!(package["source"]["reference"], serde_json::Value::Null);
}

#[test]
fn a_git_ref_prefers_branch_then_tag_then_commit() {
    // Naming more than one is not an error, so the order decides. Without it a
    // spec carrying both a branch and a tag would resolve differently on a whim.
    assert!(matches!(
        source(r#""acme/lib" = { url = "https://x/y", branch = "main", tag = "v1", reference = "abc" }"#),
        Source::Git { reference: GitRef::Branch(branch), .. } if branch == "main",
    ));
    assert!(matches!(
        source(r#""acme/lib" = { url = "https://x/y", tag = "v1", reference = "abc" }"#),
        Source::Git { reference: GitRef::Tag(tag), .. } if tag == "v1",
    ));
}

#[test]
fn the_shapes_are_tried_in_order() {
    // A spec setting several fields is not ambiguous: `repository` is checked
    // first, then `path`, and only what is left has to carry a `url`.
    assert!(matches!(
        source(r#""acme/lib" = { repository = "internal", path = "packages/lib" }"#),
        Source::Repository { .. },
    ));
    assert!(matches!(
        source(r#""acme/lib" = { path = "packages/lib", url = "https://x/y" }"#),
        Source::Path { .. },
    ));
}

// ---------------------------------------------------------------------------
// Rejections
// ---------------------------------------------------------------------------

#[test]
fn a_spec_naming_no_shape_is_an_error() {
    let err = sources(r#""acme/lib" = { version = "1.0.0" }"#).unwrap_err();
    assert!(err.contains("acme/lib"), "got: {err}");
    assert!(err.contains("repository"), "got: {err}");
}

#[test]
fn symlink_true_is_accepted() {
    // `true` restates the only supported behavior, so it loads unchanged.
    assert!(matches!(
        source(r#""acme/lib" = { path = "packages/lib", symlink = true }"#),
        Source::Path { .. },
    ));
}

#[test]
fn symlink_false_is_rejected_rather_than_ignored() {
    // Copying is not implemented, so honoring the flag would mean symlinking anyway,
    // the opposite of what was asked. Fail loudly instead.
    let err = sources(r#""acme/lib" = { path = "packages/lib", symlink = false }"#).unwrap_err();
    assert!(err.contains("acme/lib"), "got: {err}");
    assert!(err.contains("symlink = false"), "got: {err}");
}
