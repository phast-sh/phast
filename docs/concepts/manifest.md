---
description: "phast.toml, the project manifest: every section Phast supports, from [require] and [autoload] to [workspace], plus schema support for editors."
---

# The manifest

`phast.toml` is the project manifest: the single source of truth for what your project
is, what it depends on, and how Phast should behave. The format keeps Composer's core
vocabulary (`require`, `require-dev`, `autoload`) so it reads familiarly, expressed as
idiomatic TOML.

An example touching every section Phast supports (most are optional; a real
application uses a handful):

```toml
#:schema https://phast.sh/schema/v1/manifest.json
[package]
name        = "acme/my-app"
version     = "1.4.0"
description = "My application"
type        = "project"
license     = "MIT"
homepage    = "https://example.com"
keywords    = ["framework", "web"]

[[package.authors]]
name  = "Acme Engineering"
email = "dev@acme.example"

[package.support]
issues = "https://github.com/acme/my-app/issues"

[[package.funding]]
type = "github"
url  = "https://github.com/sponsors/acme"

[require]
php                 = "^8.2"
"laravel/framework" = "^11.0"
"psr/log"           = "^3.0"

[require-dev]
"phpunit/phpunit" = "^11.0"

[provide]
"psr/log-implementation" = "3.0"

[replace]
"acme/legacy" = "self.version"

[conflict]
"acme/old-sdk" = "<2.0"

[suggest]
"ext-redis" = "For faster caching"

[autoload.psr-4]
"App\\" = ["src/"]

[autoload]
files   = ["src/helpers.php"]
exclude = ["src/Legacy/"]

[autoload-dev.psr-4]
"Tests\\" = ["tests/"]

[scripts]
test             = "@php vendor/bin/phpunit"
post-install-cmd = ["@php scripts/post-install.php"]

[config]
minimum-package-age = "24h"
minimum-stability   = "stable"
prefer-stable       = true
sort-packages       = true
secure-http         = true

[config.platform]
php      = "8.2"
ext-intl = "1.0"

[[repositories]]
name = "internal"
type = "composer"
url  = "https://packages.acme.example"

[sources]
"acme/internal-lib" = { repository = "internal" }

[workspace]
members = ["libs/*"]

[workspace.dependencies]
"laravel/framework" = "^11.0"

[extra]
branch-alias = { "dev-main" = "1.x-dev" }
```

## `[package]`

Project identity: `name` (`vendor/package` format), `version`, `description`, `type`
(`library`, `project`, or `plugin`), `license` (SPDX identifier), `homepage`, and
`keywords`. All optional for applications; `name` is needed when the package is required
by others, and `version` matters for [workspace](../guides/workspaces.md) members that
siblings depend on. Three informational sub-tables are carried through and preserved but
not interpreted by the resolver: `[[package.authors]]`, `[package.support]` (channel name
to URL), and `[[package.funding]]`.

## `[require]` and `[require-dev]`

Package names mapped to version constraints. `[require-dev]` entries are skipped by
`--no-dev` installs and excluded from production autoloading. A package belongs in one
section, never both.

Three kinds of keys are valid:

- **Registry packages**: `vendor/package` names resolved from Packagist or your
  configured registries.
- **Platform packages**: `php`, `php-64bit`, and `ext-*` describe the runtime rather
  than installable code. Phast checks them against your detected PHP (or the
  `[config.platform]` overrides) during resolution.
- **Workspace references**: `{ workspace = true }` inherits the constraint from
  `[workspace.dependencies]` at the workspace root; see [Workspaces](../guides/workspaces.md).

Constraint syntax is Composer's (`^`, `~`, wildcards, ranges, `||`); the exact semantics
are covered in [Resolution](resolution.md). A value may also be the literal
`self.version`, which pins the dependency to the declaring package's own `version`
(the same sentinel Composer uses in `require`, `require-dev`, and `conflict`).

A constraint may carry a trailing `@stability` flag (`"^2.0@dev"`, `"@beta"`) that sets
the stability floor for that one package, overriding `minimum-stability` without
loosening it for anything else; see
[Per-package stability](resolution.md#per-package-stability).

## `[autoload]` and `[autoload-dev]`

Autoloading rules for your own code, compiled into `vendor/autoload.php` together with
every dependency's rules:

```toml
[autoload.psr-4]
"App\\" = ["src/"]               # namespace prefix -> base directory (or list of them)

[autoload]
classmap = ["database/"]         # directories/files scanned for class definitions
files    = ["src/helpers.php"]   # required on every request
exclude  = ["src/Legacy/"]       # paths kept out of the generated classmap
```

`psr-4` is the strategy for modern code; `psr-0`, `classmap`, and `files` exist for
legacy layouts and global helpers. `[autoload-dev]` has the same shape and only applies
when dev dependencies are installed (typically your test namespace).

`exclude` lists paths whose classes must not end up in the generated classmap. Use it for
files that are not safe to have indexed: a fixture that intentionally declares a broken
class, a stub only loaded under a specific PHP version, a vendored copy of a class that
also exists elsewhere.

It covers your `psr-4` and `psr-0` roots as well as your `classmap` entries, because
Phast indexes every declared root into the classmap so class lookups cost one hash hit
instead of a filesystem probe. Excluding a path is what keeps a file out of that index.
Patterns are relative to your project:

| Pattern             | Matches                                                 |
|---------------------|---------------------------------------------------------|
| `src/Legacy/`       | everything under that directory, at any depth           |
| `/tests/`           | a leading `/` anchors at the project root               |
| `Fixtures/`         | any directory named `Fixtures`, anywhere in the tree    |
| `src/**/*.stub.php` | `*` matches within a path segment, `**` across segments |

Dependencies declare their own exclusions the same way and Phast honors them. A
package's patterns only ever apply to its own files, so one dependency can never
suppress another's classes.

## `[scripts]`

Lifecycle hooks and named tasks; a string or an array of strings per entry. Covered in
[Running scripts](../guides/scripts.md).

## `[config]`

Settings that change how Phast resolves and installs:

| Key                      | Default    | Meaning                                                        |
|--------------------------|------------|----------------------------------------------------------------|
| `minimum-stability`      | `"stable"` | Lowest release stability the resolver may select               |
| `prefer-stable`          | `false`    | Accepted for Composer compatibility                            |
| `sort-packages`          | `false`    | Keep `[require]` sorted on every write                         |
| `minimum-package-age`    | unset      | Exclude versions newer than e.g. `"24h"`, `"7d"`, `"2w"`       |
| `secure-http`            | `true`     | Reject plaintext `http://` registry URLs; set `false` to allow |
| `[config.platform]`      | unset      | Override the PHP/extension versions used during resolution     |
| `[config.allow-plugins]` | unset      | Reserved; plugin execution is not implemented yet              |

`[config.platform]` is the reproducibility tool: with `php = "8.2"` resolution behaves
identically on every machine regardless of the locally installed PHP, and it is the way
to resolve on a machine that does not have PHP (or some production extension) installed
at all:

```toml
[config.platform]
php      = "8.2"
ext-intl = "1.0"
```

`minimum-package-age` is the supply-chain guard, explained in
[Security](../guides/security.md#guarding-against-fresh-supply-chain-attacks).

## `[[repositories]]` and `[sources]`

`[[repositories]]` declares multi-package endpoints (extra registries beyond Packagist,
a path glob, a VCS discovery source); `[sources]` attaches a single package to a source
and pins it there:

```toml
[[repositories]]
name = "nova"
type = "composer"
url  = "https://nova.laravel.com"

[sources]
"laravel/nova"   = { repository = "nova" }                                   # a named endpoint
"acme/io"        = { url = "https://github.com/acme/io", branch = "main" }   # single-package git
"acme/contracts" = { path = "packages/contracts", version = "3.7.x-dev" }    # local path
```

A `[sources]` value is an inline table, one of:

- **Named repository**: `{ repository = "nova" }`, naming any `[[repositories]]` entry
  that declares a `name`, whether it is a registry or a `vcs` endpoint
- **Local path**: `{ path, symlink?, version? }` (`symlink` may only be `true`)
- **Inline self-describing package**: `{ url, version, reference? }`
- **Single-package git source**: `{ url, branch|tag|reference }`

Covered in detail in [Registries and authentication](registries.md).

## `[workspace]`

Marks the manifest as a workspace root and declares its members; covered in
[Workspaces](../guides/workspaces.md).

## `[provide]`, `[replace]`, `[conflict]`

Package relation declarations with Composer semantics, honored during resolution
whether they come from a dependency or the root project itself.

`[conflict]` excludes versions of the named packages matching the constraint: they are
never selected, and resolution fails with an explanation if a dependency strictly
requires one.

`[replace]` shadows a package: the named package is treated as already supplied by the
declaring package and is never installed separately. A monorepo metapackage uses this to
stand in for the components it bundles.

`[provide]` declares a virtual package that an implementation satisfies, e.g. a
`psr/log-implementation` interface that a logger fulfills.

`[replace]` and `[provide]` values are version *specs*, not ranges: an exact version,
`*`, or `self.version` (the declaring package's own version). On the root project,
`self.version` and `*` resolve to the root's declared `version`, falling back to a
synthetic version when none is set.

## `[suggest]`

Optional packages mapped to a human-readable reason (`"ext-redis" = "For faster
caching"`). Purely informational: preserved from `composer.json` and carried through, but
never installed or resolved.

## `[extra]`

Free-form data for framework tooling. Phast preserves it verbatim but does not interpret
it.

## Schema and editor support

`phast.toml` has a JSON Schema, generated from Phast's own types and published at
`https://phast.sh/schema/v1/manifest.json`. The `#:schema` line at the top of the example
above binds a file to it. `phast init` and `phast migrate` write that line into the
manifests they generate, so editors that validate TOML against JSON Schema (for example
VS Code with the Even Better TOML extension) give you key completion and inline validation
out of the box; add the same line by hand to a manifest that lacks it.

The schema checks the file's shape: which keys exist, their types, and closed value sets
(for example `minimum-stability` must be one of `stable`/`rc`/`beta`/`alpha`/`dev`, and
`minimum-package-age` a duration like `24h`). It does not check Phast's deeper semantics,
whether a version constraint is well-formed, whether a `[sources]` entry names a real
repository, or whether the graph resolves; those are Phast's own job at install time.

## Formatting

`phast.toml` has a canonical style (quoted keys as basic strings, multiline script
arrays, one blank line between sections) that every Phast write preserves, including
your comments. `phast fmt` normalizes hand-edited files; `phast fmt --check` verifies in
CI.
