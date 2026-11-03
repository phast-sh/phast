use std::collections::BTreeMap;

use phast_core::lock::hash;
use phast_core::lock::phast::CURRENT_VERSION;
use phast_core::lock::phast::{
    AutoloadSection, DistSection, DistType, MetadataSection, PackageDef, PhastLock,
};
use phast_core::lock::sort;
use phast_core::manifest::{Manifest, Repository};
use phast_core::package::PackageName;
use phast_version::RequireSpec;
use phast_version::Stability;
use phast_version::parser::parse_constraint;

/// A manifest requiring nothing and configuring nothing, so any hash difference
/// a test sees comes from what that test changed.
fn manifest() -> Manifest {
    Manifest::default()
}

/// A lock holding one production package and no dev ones.
///
/// The package carries a dist and a PSR-4 rule, so a round-trip has optional
/// sections it could lose rather than only the required ones.
fn lock() -> PhastLock {
    PhastLock {
        version: CURRENT_VERSION,
        content_hash: "sha256:deadbeef".to_owned(),
        metadata: MetadataSection {
            generated_by: "phast 0.1.0".to_owned(),
            generated_at: "2026-04-08T12:00:00Z".to_owned(),
        },
        packages: vec![PackageDef {
            name: "vendor/foo".to_owned(),
            version: "1.2.3".to_owned(),
            dist: Some(DistSection {
                dist_type: DistType::Zip,
                url: "https://example.com/foo-1.2.3.zip".to_owned(),
                reference: Some("abc123".to_owned()),
                shasum: Some("cafebabe".to_owned()),
            }),
            package_type: Some("library".to_owned()),
            autoload: AutoloadSection {
                psr4: {
                    let mut m = BTreeMap::new();
                    m.insert("Vendor\\Foo\\".to_owned(), vec!["src/".to_owned()]);
                    m
                },
                ..Default::default()
            },
            ..Default::default()
        }],
        packages_dev: vec![],
        autoload: Default::default(),
        autoload_dev: Default::default(),
    }
}

/// A workspace member `name` at `version`, which a `path` is what marks it as.
fn member(name: &str, version: &str) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: version.to_owned(),
        path: Some(format!("packages/{name}")),
        ..Default::default()
    }
}

/// A zip dist at `url`, carrying `reference` and no checksum.
fn dist(url: &str, reference: Option<&str>) -> DistSection {
    DistSection {
        dist_type: DistType::Zip,
        url: url.to_owned(),
        reference: reference.map(str::to_owned),
        shasum: None,
    }
}

// ---------------------------------------------------------------------------
// What moves the content hash
// ---------------------------------------------------------------------------

#[test]
fn content_hash_is_stable() {
    let manifest = manifest();
    let h1 = hash::compute(&manifest, &[]).unwrap();
    let h2 = hash::compute(&manifest, &[]).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn content_hash_starts_with_sha256() {
    let manifest = manifest();
    assert!(
        hash::compute(&manifest, &[])
            .unwrap()
            .starts_with("sha256:")
    );
}

#[test]
fn content_hash_changes_when_require_changes() {
    let mut a = manifest();
    let mut b = manifest();
    b.require.insert(
        PackageName::new("vendor/foo"),
        RequireSpec::Constraint {
            constraint: parse_constraint("^1.0").unwrap(),
            stability: None,
        },
    );

    assert_ne!(
        hash::compute(&a, &[]).unwrap(),
        hash::compute(&b, &[]).unwrap()
    );

    a.require.insert(
        PackageName::new("vendor/foo"),
        RequireSpec::Constraint {
            constraint: parse_constraint("^1.0").unwrap(),
            stability: None,
        },
    );
    assert_eq!(
        hash::compute(&a, &[]).unwrap(),
        hash::compute(&b, &[]).unwrap()
    );
}

#[test]
fn content_hash_ignores_description_and_name() {
    let mut a = manifest();
    let b = manifest();
    a.name = Some("namespace/app".to_owned());
    a.description = Some("My app".to_owned());
    assert_eq!(
        hash::compute(&a, &[]).unwrap(),
        hash::compute(&b, &[]).unwrap()
    );
}

#[test]
fn content_hash_changes_with_minimum_stability() {
    let mut relaxed = manifest();
    relaxed.config.minimum_stability = Stability::Dev;

    // `[config]` is part of the input, so loosening what may be selected
    // invalidates a lock resolved under the stricter floor.
    assert_ne!(
        hash::compute(&manifest(), &[]).unwrap(),
        hash::compute(&relaxed, &[]).unwrap(),
    );
}

#[test]
fn content_hash_changes_with_a_platform_override() {
    let mut pinned = manifest();
    pinned
        .config
        .platform
        .insert("php".to_owned(), "8.2.0".to_owned());

    assert_ne!(
        hash::compute(&manifest(), &[]).unwrap(),
        hash::compute(&pinned, &[]).unwrap(),
    );
}

#[test]
fn content_hash_changes_when_repositories_are_reordered() {
    let repository = |url: &str| Repository::Composer {
        name: None,
        url: url.to_owned(),
        default: false,
    };

    let mut first = manifest();
    first.repositories = vec![
        repository("https://a.example"),
        repository("https://b.example"),
    ];
    let mut second = manifest();
    second.repositories = vec![
        repository("https://b.example"),
        repository("https://a.example"),
    ];

    // Repositories are the one part of the input kept in declaration order,
    // because that order is resolution priority: swapping them can change what
    // resolves, so it has to dirty the lock.
    assert_ne!(
        hash::compute(&first, &[]).unwrap(),
        hash::compute(&second, &[]).unwrap(),
    );
}

// ---------------------------------------------------------------------------
// What a workspace member adds to the hash
// ---------------------------------------------------------------------------

#[test]
fn adding_member_changes_hash() {
    let manifest = manifest();
    let no_members = hash::compute(&manifest, &[]).unwrap();
    let with_member = hash::compute(&manifest, &[member("acme/core", "1.0.0")]).unwrap();
    assert_ne!(no_members, with_member);
}

#[test]
fn member_version_bump_changes_hash() {
    let manifest = manifest();
    let h1 = hash::compute(&manifest, &[member("acme/core", "1.0.0")]).unwrap();
    let h2 = hash::compute(&manifest, &[member("acme/core", "1.1.0")]).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn member_rename_changes_hash() {
    let manifest = manifest();
    let h1 = hash::compute(&manifest, &[member("acme/core", "1.0.0")]).unwrap();
    let h2 = hash::compute(&manifest, &[member("acme/kernel", "1.0.0")]).unwrap();
    assert_ne!(h1, h2);
}

#[test]
fn member_order_does_not_affect_hash() {
    let manifest = manifest();
    let a = hash::compute(
        &manifest,
        &[member("acme/alpha", "1.0.0"), member("acme/beta", "2.0.0")],
    )
    .unwrap();
    let b = hash::compute(
        &manifest,
        &[member("acme/beta", "2.0.0"), member("acme/alpha", "1.0.0")],
    )
    .unwrap();
    assert_eq!(a, b);
}

#[test]
fn member_require_change_changes_hash() {
    let manifest = manifest();
    let base = member("acme/core", "1.0.0");
    let mut with_dep = base.clone();
    with_dep
        .require
        .entries
        .insert("php".to_owned(), "^8.1".to_owned());

    let h1 = hash::compute(&manifest, &[base]).unwrap();
    let h2 = hash::compute(&manifest, &[with_dep]).unwrap();
    assert_ne!(h1, h2);
}

// ---------------------------------------------------------------------------
// What marks a lock entry as a workspace member
// ---------------------------------------------------------------------------

#[test]
fn is_member_true_when_path_present() {
    let pkg = PackageDef {
        name: "acme/core".to_owned(),
        version: "1.0.0".to_owned(),
        path: Some("packages/core".to_owned()),
        ..Default::default()
    };
    assert!(pkg.is_member());
}

#[test]
fn is_member_false_when_path_absent() {
    let pkg = PackageDef {
        name: "vendor/foo".to_owned(),
        version: "1.2.3".to_owned(),
        ..Default::default()
    };
    assert!(!pkg.is_member());
}

// ---------------------------------------------------------------------------
// phast.lock round-trip
// ---------------------------------------------------------------------------

#[test]
fn phast_lock_round_trips() {
    let lock = lock();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("phast.lock");

    lock.write(&path).unwrap();
    let read_back = PhastLock::read(&path).unwrap();

    assert_eq!(read_back.version, lock.version);
    assert_eq!(read_back.content_hash, lock.content_hash);
    assert_eq!(read_back.packages.len(), 1);
    assert_eq!(read_back.packages[0].name, "vendor/foo");
    assert_eq!(read_back.packages[0].version, "1.2.3");
    assert!(read_back.packages[0].dist.is_some());
}

#[test]
fn dist_type_path_round_trips() {
    let dist = DistSection {
        dist_type: DistType::Path,
        url: "../packages/local-lib".to_owned(),
        reference: None,
        shasum: None,
    };

    let toml = toml::to_string(&dist).unwrap();
    assert!(toml.contains(r#"type = "path""#), "serialized as: {toml}");

    let read_back: DistSection = toml::from_str(&toml).unwrap();
    assert_eq!(read_back, dist);
    assert_eq!(read_back.dist_type, DistType::Path);
}

#[test]
fn phast_lock_is_fresh_check() {
    let lock = lock();
    assert!(lock.is_fresh("sha256:deadbeef"));
    assert!(!lock.is_fresh("sha256:other"));
}

#[test]
fn download_url_resolves_pretty_version() {
    let dist = dist("https://host/archive/%prettyVersion%.zip", Some("abc123"));
    assert_eq!(
        dist.download_url("v2.0.0"),
        "https://host/archive/v2.0.0.zip"
    );
}

#[test]
fn download_url_resolves_reference() {
    let dist = dist("https://host/archive/%reference%.zip", Some("abc123"));
    assert_eq!(
        dist.download_url("v2.0.0"),
        "https://host/archive/abc123.zip"
    );
}

#[test]
fn download_url_leaves_plain_urls_unchanged() {
    let url = "https://host/zipball/abc123";
    assert_eq!(dist(url, Some("abc123")).download_url("v2.0.0"), url);
}

// ---------------------------------------------------------------------------
// Topological sort
// ---------------------------------------------------------------------------

/// A package named `name` that requires each of `requires`.
fn package(name: &str, requires: &[&str]) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        require: requires
            .iter()
            .map(|r| ((*r).to_owned(), "*".to_owned()))
            .collect::<BTreeMap<String, String>>()
            .into(),
        ..Default::default()
    }
}

#[test]
fn sort_puts_dependencies_before_their_dependents() {
    let sorted = sort::kahn(vec![
        package("acme/app", &["acme/mid"]),
        package("acme/mid", &["acme/base"]),
        package("acme/base", &[]),
    ]);

    let order: Vec<&str> = sorted.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(order, ["acme/base", "acme/mid", "acme/app"]);
}

#[test]
fn a_cycle_keeps_every_package() {
    let sorted = sort::kahn(vec![
        package("acme/app", &["acme/lib"]),
        package("acme/lib", &["acme/app"]),
        package("acme/free", &[]),
    ]);

    // Neither side of a cycle ever reaches zero unresolved dependencies, so they
    // are appended after what did resolve rather than dropped from the lock.
    let order: Vec<&str> = sorted.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(order, ["acme/free", "acme/app", "acme/lib"]);
}

#[test]
fn sort_is_independent_of_input_order() {
    // Packages with no edge between them are ordered by the caller's iteration
    // order unless the sort pins them. `enrich` walks a HashMap, so that order is
    // randomized per process: without a tiebreak the lock file reshuffles between
    // runs on an unchanged manifest.
    let names = ["psr/log", "psr/container", "psr/simple-cache"];
    let expected: Vec<String> = {
        let mut sorted = names.map(str::to_owned).to_vec();
        sorted.sort();
        sorted
    };

    for rotation in 0..names.len() {
        let mut input = names.to_vec();
        input.rotate_left(rotation);

        let sorted = sort::kahn(input.iter().map(|n| package(n, &[])).collect());
        let order: Vec<String> = sorted.iter().map(|p| p.name.clone()).collect();
        assert_eq!(order, expected, "input rotated by {rotation}");
    }
}
