---
description: "Environment variables Phast reads, such as PHAST_PHP_BIN and PHAST_CACHE_DIR, and how to set them per project."
---

# Environment variables

All environment variables Phast reads.

## `PHAST_PHP_BIN`

Path to the PHP interpreter Phast uses. Affects everything that touches PHP: platform
detection (PHP version and loaded extensions during resolution), `@php` script
commands, and PHP static callbacks.

Default: `php` on your `PATH`.

```console
$ export PHAST_PHP_BIN=/opt/homebrew/opt/php@8.3/bin/php
```

Useful with PHP version managers, or to test a project against a different interpreter
without changing your shell `PATH`.

## `PHAST_CACHE_DIR`

Overrides the cache directory for metadata and downloaded archives. Equivalent to
passing `--cache-dir` on every command (the flag wins when both are set).

Default: the platform cache directory (`~/.cache/phast` on Linux,
`~/Library/Caches/phast` on macOS, `%LOCALAPPDATA%\phast` on Windows).

```console
$ export PHAST_CACHE_DIR=.phast-cache
```

Primarily for CI pipelines, where the cache must live at a path the CI system can
persist between jobs; see [Caching](../concepts/caching.md).

## `PHAST_HTTP_CONTACT`

A contact email included in the `User-Agent` header of every registry request, in the
`mailto=` form Packagist recommends for high-volume clients:

```console
$ export PHAST_HTTP_CONTACT=you@example.com
```

Optional and purely cooperative: it lets registry operators reach you if your traffic
pattern ever looks problematic, instead of blocking it.

## Per-project settings

To set variables for one project only, use a tool such as [direnv](https://direnv.net),
which loads them when you enter the directory, once you have allowed it:

```sh
# .envrc, loaded by direnv after `direnv allow`
export PHAST_PHP_BIN=/opt/homebrew/opt/php@8.3/bin/php
export PHAST_CACHE_DIR=.phast-cache
```

In CI, set them in the job's environment.

## Variables read by your scripts

Scripts run from `[scripts]` inherit Phast's full environment, including everything
above, so a script can rely on the same `PHAST_PHP_BIN` that Phast itself resolved.
