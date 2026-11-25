use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use phast_autoload::builder::{Collect, Sources};
use phast_core::lock::phast::{AutoloadSection, MetadataSection, PackageDef, PhastLock};
use phast_workspace::MemberAutoload;

use anyhow::Result;

/// A lock with no packages and no root autoload rules, for a test to fill in.
fn lock() -> PhastLock {
    PhastLock {
        version: 1,
        content_hash: "test".to_owned(),
        metadata: MetadataSection {
            generated_by: "test".to_owned(),
            generated_at: "2026-01-01T00:00:00Z".to_owned(),
        },
        packages: vec![],
        packages_dev: vec![],
        autoload: Default::default(),
        autoload_dev: Default::default(),
    }
}

/// A package `name` whose only rule is the PSR-4 `prefix`.
fn package(name: &str, prefix: &str) -> PackageDef {
    PackageDef {
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        autoload: psr4(prefix),
        ..Default::default()
    }
}

/// A member at `dir`, with `prefix` as its rule and `dev_prefix` as its dev rule.
fn member(dir: &str, prefix: &str, dev_prefix: &str) -> MemberAutoload {
    MemberAutoload {
        dir: PathBuf::from(dir),
        autoload: psr4(prefix),
        autoload_dev: psr4(dev_prefix),
    }
}

/// A section whose only rule is the PSR-4 `prefix`, mapped to `src/`.
fn psr4(prefix: &str) -> AutoloadSection {
    AutoloadSection {
        psr4: BTreeMap::from([(prefix.to_owned(), vec!["src/".to_owned()])]),
        ..Default::default()
    }
}

/// A lock with a package, a dev package and root rules, plus one workspace member.
#[rustfmt::skip]
fn project() -> (PhastLock, Vec<MemberAutoload>) {
    let mut lock = lock();
    lock.packages.push(package(
        "acme/hooks",
        "Acme\\Hooks\\",
    ));
    lock.packages_dev.push(package(
        "acme/testing",
        "Acme\\Testing\\",
    ));

    lock.autoload = psr4("App\\");
    lock.autoload_dev = psr4("App\\Tests\\");

    let members = vec![member(
        "/project/packages/api",
        "Api\\",
        "Api\\Tests\\",
    )];
    (lock, members)
}

/// Records each source it gets, as its directory and PSR-4 prefix.
#[derive(Default)]
struct Recorded {
    sources: Vec<(PathBuf, String)>,
}

#[rustfmt::skip]
impl Collect for Recorded {
    type Output = Self;

    fn add(&mut self, autoload: &AutoloadSection, dir: &Path) -> Result<()> {
        let prefix = autoload
            .psr4
            .keys()
            .next()
            .cloned()
            .unwrap_or_default();
        self.sources.push((dir.to_path_buf(), prefix));
        Ok(())
    }

    fn finish(self) -> Self {
        self
    }
}

// ---------------------------------------------------------------------------
// Which sources come, in which order
// ---------------------------------------------------------------------------

#[test]
#[rustfmt::skip]
fn packages_come_first_then_members_then_the_root() {
    let (lock, members) = project();

    let sources = Sources::new(Path::new("/project/vendor"), false, Recorded::default())
        .collect(&lock, &members)
        .unwrap()
        .sources;

    assert_eq!(
        sources,
        [
            (PathBuf::from("/project/vendor/acme/hooks"), "Acme\\Hooks\\".to_owned()),
            (PathBuf::from("/project/packages/api"), "Api\\".to_owned()),
            (PathBuf::from("/project"), "App\\".to_owned()),
        ],
    );
}

#[test]
#[rustfmt::skip]
fn dev_rules_follow_their_source_with_include_dev() {
    let (lock, members) = project();

    // Dev packages come after the regular ones. A member's or the root's dev rules come
    // right after its regular rules.
    let sources = Sources::new(Path::new("/project/vendor"), true, Recorded::default())
        .collect(&lock, &members)
        .unwrap()
        .sources;

    assert_eq!(
        sources,
        [
            (PathBuf::from("/project/vendor/acme/hooks"), "Acme\\Hooks\\".to_owned()),
            (PathBuf::from("/project/vendor/acme/testing"), "Acme\\Testing\\".to_owned()),
            (PathBuf::from("/project/packages/api"), "Api\\".to_owned()),
            (PathBuf::from("/project/packages/api"), "Api\\Tests\\".to_owned()),
            (PathBuf::from("/project"), "App\\".to_owned()),
            (PathBuf::from("/project"), "App\\Tests\\".to_owned()),
        ],
    );
}
