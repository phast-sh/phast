---
description: "How Phast turns constraints into exact versions with PubGrub: Composer's constraint syntax, stability, platform packages, and readable conflict reports."
---

# Resolution

Resolution turns the constraints in `phast.toml` into the exact versions in `phast.lock`.
Phast uses the [PubGrub](https://github.com/pubgrub-rs/pubgrub) algorithm (the same family
as UV and Dart's pub): it learns from every conflict it encounters, never re-explores a
dead end, and when no solution exists it can explain *why* instead of dumping solver
internals.

## Constraint syntax

Phast supports Composer's full constraint language:

| Constraint        | Meaning                                    | Equivalent range                       |
|-------------------|--------------------------------------------|----------------------------------------|
| `^1.2.3`          | Compatible, no breaking changes            | `>=1.2.3 <2.0.0`                       |
| `^0.3.4`          | Caret on a 0.x line                        | `>=0.3.4 <0.4.0`                       |
| `~1.2.3`          | Patch-level changes                        | `>=1.2.3 <1.3.0`                       |
| `~1.2`            | Minor-level changes                        | `>=1.2.0 <2.0.0`                       |
| `1.2.*` / `1.2.x` | Wildcard                                   | `>=1.2.0 <1.3.0`                       |
| `>=1.0 <2.0`      | Range (space or comma is AND)              | as written                             |
| `1.0 - 2.0`       | Hyphen range                               | `>=1.0 <2.1`                           |
| `1.0 \|\| 2.0`    | OR                                         | union                                  |
| `!=1.5`           | Exclusion                                  | everything but 1.5                     |
| `*`               | Anything                                   | full range                             |
| `dev-main`        | A branch, matched exactly                  | that branch only                       |
| `^2.0@dev`        | Range + a stability floor for this package | `>=2.0.0 <3.0.0`, dev releases allowed |

Notes that trip people up:

- **Caret on 0.x**: `^0.3.4` locks the *minor* version, because the 0.x line is
  unstable by convention. This matches Composer (and differs from npm).
- **Tilde precision**: `~1.2.3` and `~1.2.0` allow up to the next minor; `~1.2` allows
  up to the next major. Whether you wrote the third segment matters, even when it is
  zero.
- **Version spellings**: `v1.2.3`, `1.2.3.4` (four segments), and stability suffixes in
  any spelling (`1.0.0-beta1`, `1.0.0-beta.1`, `1.0.0beta1`) all parse and compare
  correctly.

## Stability

Every version has a stability derived from its suffix, ordered
`dev < alpha < beta < RC < stable`. The resolver only ever sees versions at or above
your `minimum-stability` (default: `stable`):

```toml
[config]
minimum-stability = "beta"   # allow beta and RC pre-releases
```

Within the allowed set, Phast picks the newest version that satisfies all constraints.

### Per-package stability

Lowering `minimum-stability` for the whole project to accommodate one package is a blunt
instrument: it opens every dependency in your tree to pre-releases. A trailing
`@stability` flag on the constraint sets the floor for that package only:

```toml
[config]
minimum-stability = "stable"     # everything else stays stable

[require]
"acme/sdk"        = "^2.0@dev"   # ...but this package may resolve to a dev version
"acme/next"       = "@beta"      # any version, as long as it is beta or better
```

The flag is not part of the version range: `"^2.0@dev"` still means "2.x", it only
changes which 2.x releases are visible. Written without a range (`"@beta"`) it means "any version,
at that stability".

Accepted labels are the same five: `dev`, `alpha`, `beta`, `rc`, `stable`. The flag
replaces the project floor for that package in either direction, so on a project with
`minimum-stability = "dev"` a `"^1.0@stable"` pins one dependency back to stable
releases.

Two limits worth knowing:

- **Only your own requirements count.** A flag in `[require]` or `[require-dev]` applies;
  a flag inside a *dependency's* own requirements is ignored. Otherwise any package in
  your tree could quietly lower your project's stability policy from a distance.
- **In a workspace it is workspace-wide.** Members share one lock and one `vendor/`, so a
  flag written by any member applies to that package for the whole workspace, exactly
  like the merged constraints do.

Adding or removing a flag changes the resolution, so it invalidates `phast.lock` and the
next `phast install` re-resolves.

## Platform packages

`php`, `php-64bit`, and `ext-*` requirements are constraints on the runtime, not
downloadable packages. Phast seeds them before resolving:

1. Explicit `[config.platform]` entries win.
2. Otherwise the local `php` binary is asked for its version and loaded extensions.

A dependency tree requiring `ext-mbstring` therefore resolves instantly against your
actual runtime, and fails with a clear conflict if the requirement cannot be met,
rather than at runtime in production.

## Virtual packages

Packages like `psr/log-implementation` or `psr/http-client-implementation` do not exist
on any registry; they are interface contracts. Any package whose metadata declares a
matching `provide` (or `replace`) satisfies them. Phast resolves these automatically
from the metadata of the real packages in your tree, with backtracking across multiple
candidate providers; no configuration needed.

The root project's own [`[provide]` / `[replace]`](manifest.md#provide-replace-conflict)
work the same way: the root satisfies the named package directly (installing nothing
extra for a `provide`, shadowing the real package for a `replace`). This covers the
monorepo metapackage pattern, where a root replaces the components it bundles at
`self.version`.

## How install, update, and lock differ

All three run the same solver with different inputs:

| Command                      | Existing lock is used as           | Effect                                          |
|------------------------------|------------------------------------|-------------------------------------------------|
| `phast install` (stale lock) | preferences                        | changed constraints move; everything else stays |
| `phast update`               | ignored                            | everything moves to the newest allowed version  |
| `phast update <pkg>`         | hard pins for all *other* packages | only the named packages can move                |
| `phast lock --refresh`       | preferences                        | forced re-resolve with minimal churn            |

A *preference* means "use this version if it still fits"; a *pin* means "this version,
or fail". Pins are what make selective updates safe: if updating your target would
force an unrequested change elsewhere, you get an error naming the conflict instead of
a surprise upgrade.

## When resolution fails

Conflicts are reported in terms of your project, with the competing requirements lined
up:

```
Phast could not find compatible versions for your requirements
 ╰─▶ symfony/polyfill-mbstring has incompatible constraints:
       · your project requires symfony/polyfill-mbstring[>=99.0, <100.0]
       · symfony/http-foundation[>=7.0, <8.0] requires symfony/polyfill-mbstring[>=1.1, <2.0]
```

Other failure shapes are reported just as directly: a package that does not exist on
any configured registry, or one that exists but has no version satisfying the combined
constraints. `phast why <package>` is the companion tool for understanding where a
problematic requirement comes from.

## Performance

Resolution speed comes from never waiting on the network when avoidable: metadata for
the dependencies of every resolved package is prefetched in the background while the
solver works, each package is fetched at most once per run, results are cached on disk
and revalidated cheaply (see [Caching](caching.md)), and a warm lock skips resolution
entirely.
