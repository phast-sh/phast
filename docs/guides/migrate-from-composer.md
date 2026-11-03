---
description: "Convert composer.json to phast.toml with phast migrate: what is carried over, how credentials move to auth.toml, and what to check afterwards."
---

# Migrating from Composer

`phast migrate` converts an existing `composer.json` into a `phast.toml`. It is a
one-time conversion, not an ongoing sync: after migration, `phast.toml` is the single
source of truth and Phast neither reads nor writes `composer.json` again. This is the
same model UV used to replace pip, and it exists to avoid the split-brain state of two
manifests that can silently drift apart.

## Run the migration

From the directory containing `composer.json`:

```console
$ phast migrate
composer.json migrated to phast.toml. Run 'phast install' to install dependencies.
$ phast install
```

If a `phast.toml` already exists, the command aborts; pass `--force` to overwrite it.

The migration is non-destructive: `composer.json` and `composer.lock` are left exactly
as they were. Keep them around as long as you like, or remove them once the team has
switched; Phast does not care either way.

## What is carried over

| composer.json                                          | phast.toml                                               |
|--------------------------------------------------------|----------------------------------------------------------|
| `name`, `description`, `type`, `license`, `keywords`   | `[package]`                                              |
| `require`                                              | `[require]` (constraints copied verbatim)                |
| `require-dev`                                          | `[require-dev]`                                          |
| `replace`, `provide`, `conflict`                       | `[replace]`, `[provide]`, `[conflict]` (copied verbatim) |
| `autoload` (psr-4, psr-0, classmap, files)             | `[autoload]`                                             |
| `autoload.exclude-from-classmap`                       | `[autoload] exclude`                                     |
| `autoload-dev`                                         | `[autoload-dev]`                                         |
| `scripts`                                              | `[scripts]`                                              |
| `minimum-stability`, `prefer-stable`                   | `[config]`                                               |
| `config.sort-packages`                                 | `[config]`                                               |
| `config.platform.php`                                  | `[config.platform]` (only when set explicitly)           |
| `repositories` (`composer`, `packagist`, `vcs`, `git`) | `[[repositories]]`                                       |
| `repositories` (`path`, `package`)                     | `[sources]` (per-package path / inline specs)            |
| `extra`                                                | `[extra]` (carried verbatim)                             |
| `config.http-basic`, `config.bearer`                   | `auth.toml` (see below)                                  |

Multi-package endpoints (`composer` / `packagist` registries, a `vcs` discovery repo)
become `[[repositories]]`. Single-package repositories become `[sources]` entries keyed
by package name: a `type: path` repo maps to a `{ path, version? }` spec (its
`options.versions` pin is preserved), and an inline `type: package` maps to a
self-describing inline package. `[require]` / `[require-dev]` keep their `composer.json`
order, with platform packages (`php`, `ext-*`) tiered first.

The generated `phast.toml` is formatted with Phast's canonical style, so it is also a
good first look at the format. See [The manifest](../concepts/manifest.md) for the full
reference.

## Credentials become auth.toml

Embedding credentials in a committed manifest is an anti-pattern, so inline credentials
from the `composer.json` `config` section are **not** copied into `phast.toml`. Instead,
`phast migrate` merges them with any existing `auth.toml`/`auth.json` and writes the
result to `./auth.toml`:

```console
Credentials were written to auth.toml. Add auth.toml to your .gitignore to keep
credentials out of version control.
```

!!! warning
    Do that. `auth.toml` belongs in `.gitignore`, next to `.env`.

Phast also keeps reading Composer's `auth.json` files (project-local and
`~/.composer/auth.json`), so existing credential setups continue to work without any
migration step. See [Registries and authentication](../concepts/registries.md).

## Why composer.lock is ignored

`phast migrate` deliberately does not convert `composer.lock`. That lock was produced by
Composer's resolver under Composer's platform assumptions; reusing it would bake those
assumptions into Phast. Running `phast install` after migration re-resolves from your
constraints and produces a clean `phast.lock`.

In practice this means your resolved versions may differ from the old `composer.lock`:
Phast picks the newest versions your constraints allow. If you depended on older
versions without expressing that in your constraints, tighten the constraints in
`phast.toml` before installing, using the old `composer.lock` as a reference.

## What is not migrated (yet)

Be aware of these gaps before switching a production project:

- **`suggest`**: informational only, dropped.
- **`config.allow-plugins`, `vendor-dir`, `bin-dir`**: not yet supported by Phast.
- **`repositories` of `type: svn`**: Subversion is not supported and is not planned, so
  these entries are dropped. If your project depends on one, that dependency will fail
  to resolve after migrating; move the package to a git host or a registry first.

## Script compatibility

Scripts are copied verbatim, and most run unchanged: `@php ...` commands, plain shell
commands, and PHP static callbacks (`Vendor\Class::method`) are all supported, and Phast
fires the common lifecycle events (`pre-/post-install-cmd`, `pre-/post-update-cmd`,
`pre-/post-autoload-dump`). Unknown event names are silently skipped, so leftover
Composer-specific hooks do no harm.

Two cases need attention:

- **Scripts that call `composer` directly** (e.g. `composer dump-autoload`) will only
  work if Composer is still installed; Phast does not emulate the `composer` CLI.
  Replace them with the Phast equivalent or an `@php` command.
- **Script references** (`"test": "@unit-tests"`-style aliases) are not supported yet;
  inline the referenced commands.

See [Running scripts](scripts.md) for the details.

## Checklist after migrating

1. `phast install`, and review the resolved versions in `phast.lock` against your old
   `composer.lock` where it matters.
2. Run your test suite and any tools under `vendor/bin/`.
3. Add `auth.toml` to `.gitignore` if it was created.
4. Commit `phast.toml` and `phast.lock`.
5. Update CI to install Phast and run `phast install` (the
   [caching guide](../concepts/caching.md) shows how to cache between jobs).
