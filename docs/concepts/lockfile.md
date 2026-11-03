---
description: "phast.lock records the exact result of a resolution: what it contains, how its content hash detects drift, and how it relates to composer.lock."
---

# The lock file

`phast.lock` records the exact outcome of a resolution: every package, its precise
version, where to download it, and how to autoload it. The manifest says what you
*want*; the lock says what you *got*. With a lock present, every `phast install` on
every machine produces the same `vendor/` without resolving anything.

!!! tip "Commit it"
    Always commit `phast.lock` for applications. It is what makes installs
    reproducible across your team, CI, and production.

## What it records

```toml
version = 1
content-hash = "sha256:9f2c..."

[metadata]
generated-by = "phast 0.1.0"
generated-at = "2026-06-12T12:00:00Z"

[[packages]]
name    = "monolog/monolog"
version = "3.6.0"
require = { "php" = ">=8.1", "psr/log" = "^2.0 || ^3.0" }

[packages.dist]
type      = "zip"
url       = "https://api.github.com/repos/Seldaek/monolog/zipball/..."
reference = "4b18b21a5527a3d5ffdac2fd35d3ab25a9597654"
shasum    = "..."

[packages.autoload.psr-4]
"Monolog\\" = ["src/Monolog"]
```

Per package: the resolved version, the dist archive (URL, VCS reference, checksum), the
declared dependencies, autoload rules, executable `bin` entries, and descriptive
metadata (description, license, authors, release time). Dev-only packages live in a
separate `packages-dev` list so `--no-dev` installs can skip them. The list is written
even by a `--no-dev` install, so the lock from a production build still installs the dev
packages on a developer's machine.

The lock also stores your *root project's* autoload rules. That makes it
self-contained: installing and generating the autoloader needs only the lock and the
archives, no manifest parsing and no registry metadata.

In a [workspace](../guides/workspaces.md), each member is recorded as a package entry
too, marked with a `path` instead of a `dist`:

```toml
[[packages]]
name    = "my-org/api-sdk"
version = "1.0.0"
path    = "packages/api-sdk"
```

A path-marked entry is never downloaded; the autoloader points straight at the member's
source tree. Recording members keeps the dependency graph in the lock complete, so a
member-scoped `phast install --member` can walk it to find exactly which external
archives that member needs.

Because dependencies and checksums are in the lock, the offline commands (`phast show`,
`phast why`) and integrity verification all work without network access.

## Freshness: the content hash

`content-hash` is a fingerprint of exactly the manifest fields that affect resolution:
`[require]`, `[require-dev]`, `minimum-stability`, `[config.platform]`, and the
repository list. At a workspace root it additionally covers each member's identity and
direct requirements, so adding a member, bumping a member's version, or editing a
member's constraints invalidates the lock too. On every install Phast recomputes it from
the current manifest:

- **Match**: the lock is authoritative; resolution is skipped.
- **Mismatch**: something resolution-relevant changed; Phast re-resolves, using the old
  lock as a set of preferences so unrelated packages keep their versions.

Editing fields that do not affect resolution (description, autoload paths, scripts)
does not invalidate the lock.

## Ordering and diffs

Packages are written in dependency order (dependencies before their dependents) with a
stable layout, so a lock diff after `phast add` or a selective `phast update` is short
and reviewable: the packages that changed, and nothing else. It also makes the file
merge-friendly: two branches adding different packages touch different parts of it, so
conflicts are rare and stay confined to the entries that disagree. Writes are atomic
(written to a temp file, then renamed), so an interrupted command never leaves a corrupt
lock.

## Relationship to composer.lock

There is none, by design. Phast neither reads nor writes `composer.lock`; after
[migration](../guides/migrate-from-composer.md) the first `phast install` produces a
fresh `phast.lock` from your constraints. The `version = 1` field at the top is the
lock format version, checked on read so an old Phast fails clearly on a newer lock
instead of misreading it.
