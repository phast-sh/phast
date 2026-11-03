---
description: "Day-to-day dependency management with Phast: installing, adding and removing packages, updating, checking for newer versions, and inspecting the tree."
---

# Working on projects

Day-to-day dependency management: installing, changing dependencies, updating, and
inspecting your dependency tree.

## Installing

```console
$ phast install
```

`phast install` is lock-driven:

- **Lock is in sync with the manifest**: resolution is skipped entirely. Phast only
  downloads packages missing from `vendor/` and regenerates the autoloader, so repeated
  installs are nearly instant.
- **Lock is stale** (you edited `phast.toml`): Phast re-resolves, but prefers the
  versions already in the lock; only packages affected by your change move.
- **No lock yet**: full resolution from scratch.

Useful flags:

| Flag             | Effect                                                             |
|------------------|--------------------------------------------------------------------|
| `--no-dev`       | Skip `[require-dev]` packages (production deploys, slim CI images) |
| `--dry-run`      | Show what would be installed without writing anything              |
| `--no-scripts`   | Skip all lifecycle script hooks                                    |
| `--registry-url` | Resolve against a single explicit registry, ignoring the manifest  |

Add `-v` for per-phase timings (resolve, lock, download, autoload) and `-vv` for
per-package detail; `-q` silences everything except errors.

## Adding and removing dependencies

```console
$ phast add monolog/monolog               # let Phast choose the constraint
$ phast add monolog/monolog:^3.0          # or state it yourself
$ phast add phpunit/phpunit:^11.0 --dev   # record it in [require-dev]
$ phast remove monolog/monolog
```

Both commands edit `phast.toml` in place, preserving your comments and formatting, then
run a full install (skip it with `--no-install`).

Given no constraint, Phast resolves the whole project with the package required and writes
`^major.minor` of the version that came out, so what lands in your manifest is a range that
already agrees with everything else you depend on:

```console
$ phast add monolog/monolog
Added monolog/monolog to require.
```

```toml
[require]
"monolog/monolog" = "^3.9"
```

The patch level is dropped on purpose: pinning it would defeat the caret, and the exact
version is recorded in `phast.lock`, which is where an exact version belongs.

Details worth knowing:

- A package can live in `[require]` or `[require-dev]`, not both. Adding a package that
  already exists in the *other* section is an error with a hint on how to move it.
- Adding a package that already exists in the target section updates its constraint and
  prints the old and new value.
- `phast remove` validates that every named package exists before touching the file, so
  a typo in one name never half-edits your manifest. Sections left empty are removed.
- In a workspace, target a specific member with `--member <path>`; see
  [Workspaces](workspaces.md).

## Updating

```console
$ phast update                    # everything, to the newest allowed versions
$ phast update monolog/monolog    # just this package (and what only it needs)
```

The two modes differ deliberately:

- **Full update** re-resolves everything freely within the constraints in `phast.toml`.
- **Selective update** pins every *other* locked package to its exact version. Only the
  named packages can change. If updating the named package is impossible without moving
  something you did not ask to update, the command fails with a conflict explanation
  instead of silently upgrading the bystander.

`phast update` also re-downloads packages whose version changed and regenerates the
autoloader. `--dry-run` previews the result; `--no-dev` and `--no-scripts` work as in
install.

To re-resolve and write the lock *without* touching `vendor/` (useful in CI or
pre-commit hooks):

```console
$ phast lock             # only resolves when the lock is stale
$ phast lock --refresh   # force re-resolution, keeping locked versions where possible
```

## Checking for newer versions

```console
$ phast outdated

Outdated packages (3):

  dompdf/dompdf        3.1.5   →  4.0.0   major
  laravel/framework    12.61   →  12.62   patch
  symfony/http-kernel  7.4.13  →  7.4.14  patch
```

The last column classifies each update:

- `patch` / `minor`: a plain `phast update` can take it.
- `major`: the newest release is outside your current constraint (or bumps the major
  version); updating requires editing the constraint in `phast.toml`.

`phast outdated` exits with code 1 when anything is outdated, so it works as a CI gate.
`--format json` produces machine-readable output, and `--no-dev` limits the report to
production dependencies.

## Inspecting what is installed

All of these read `phast.lock` only; they work offline and never touch `vendor/`.

List everything, or show one package in detail:

```console
$ phast show
$ phast show symfony/http-foundation

name     : symfony/http-foundation
version  : 7.2.0
type     : library
license  : MIT

requires
  php                        >=8.2
  symfony/polyfill-mbstring  ^1.1
```

Answer "why is this installed?":

```console
$ phast why psr/log

psr/log 3.0.2 is required by:

  root (your project)  ^3.0   [require]
  laravel/framework    ^12.0  [require]
  monolog/monolog      ^3.0   [require]
```

`phast why --tree` renders every reverse-dependency path up to your project root, which
is the fastest way to find out what to remove to get rid of a transitive dependency.
`--no-dev` restricts the trace to production dependencies.

`phast show --format json` and the other `--format json` outputs are stable shapes meant
for scripting.

## Keeping the manifest tidy

Every Phast command that writes `phast.toml` formats it on the way out. For hand-edited
files there is:

```console
$ phast fmt           # rewrite phast.toml in canonical style
$ phast fmt --check   # CI mode: exit 1 (with a diff) if it would change
```

In a workspace, `fmt` covers the root and every member, so one command keeps the whole
repository consistent. Each member is reported by its `[package].name`; the root has no
package name to report, since a workspace root declares none. `--member <path>` narrows it
to one manifest, and so does running it from inside a member's directory. `phast validate`
covers a workspace the same way.

Set `sort-packages = true` under `[config]` to keep `[require]` sorted automatically,
platform packages (`php`, `ext-*`) first, then alphabetically.
