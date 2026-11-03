---
description: "Create a PHP project with phast init, add a dependency, install it and use it: a first walkthrough of the commands you will run every day."
---

# First steps

This walkthrough creates a new package from scratch, adds a dependency, and installs it.
If you already have a Composer project, start with
[Migrating from Composer](../guides/migrate-from-composer.md) instead.

## Create a project

`phast init` scaffolds a new package interactively. Run it in an empty directory, or
give it a path to create one:

```console
$ phast init demo-app
```

The wizard asks for a package name (defaulting to `<username>/<directory>`), a package
type (`library`, `project`, or `plugin`), an optional description, a PHP version
constraint (defaulting to the PHP it detects on your machine), and whether to create
`src/` and `tests/` directories.

The result is a `phast.toml`, the single manifest Phast works with:

```toml
[package]
name        = "acme/demo-app"
description = "My demo application"
type        = "project"

[require]
php = "^8.3"

[autoload.psr-4]
"Acme\\DemoApp\\" = ["src/"]

[autoload-dev.psr-4]
"Tests\\" = ["tests/"]

[config.platform]
php = "8.3"
```

The PSR-4 namespace is derived from the package name, and the detected PHP version is
recorded under `[config.platform]` so resolution stays reproducible on machines with a
different PHP. The format is explained in detail in
[The manifest](../concepts/manifest.md).

## Add a dependency

```console
$ phast add monolog/monolog:^3.0
```

`phast add` writes the constraint into `[require]` (preserving any comments and
formatting in your manifest), then runs a full install. A few variations:

```console
$ phast add monolog/monolog                     # let Phast pick the constraint
$ phast add psr/log:^3.0 monolog/monolog:^3.0   # several at once
$ phast add phpunit/phpunit:^11.0 --dev         # into [require-dev]
$ phast add psr/log:^3.0 --no-install           # edit the manifest only
```

!!! tip
    Leave the constraint off and Phast chooses one: it resolves your project with the
    package required and writes `^major.minor` of the version that came out. That is not
    the newest release but the newest one that fits everything else you depend on, so the
    constraint it writes is one that resolves.

To remove a dependency again, use the mirror command:

```console
$ phast remove monolog/monolog
```

## Install

```console
$ phast install
```

On the first run Phast resolves your requirements against Packagist, then:

- writes **`phast.lock`**, the exact resolved set of every package and its download
  source. Commit this file; it is what makes installs reproducible across machines.
- populates **`vendor/`** with the package code and `vendor/bin/` with executable stubs.
- generates **`vendor/autoload.php`**, fully compatible with the autoloader your PHP
  application already expects, including the `\Composer\InstalledVersions` runtime API.

On subsequent runs, `phast install` checks that `phast.lock` is still in sync with
`phast.toml` and skips resolution entirely when it is, only fetching whatever is missing
from `vendor/`.

## Use it

Require the autoloader exactly as you would in any PHP project:

```php
<?php

require __DIR__ . '/vendor/autoload.php';

$log = new Monolog\Logger('demo');
$log->pushHandler(new Monolog\Handler\StreamHandler('php://stdout'));
$log->info('Phast works!');
```

```console
$ php demo.php
```

## A few commands you will use daily

| Command                      | What it does                                                 |
|------------------------------|--------------------------------------------------------------|
| `phast install`              | Install from the lock file, resolving first only when needed |
| `phast add` / `phast remove` | Edit `[require]` / `[require-dev]` and re-install            |
| `phast update [package...]`  | Move to the newest versions your constraints allow           |
| `phast outdated`             | Show which locked packages have newer releases               |
| `phast show [package]`       | Inspect installed packages                                   |
| `phast why <package>`        | Explain which packages require a given package               |
| `phast run <script>`         | Run a named script from `[scripts]`                          |

## Next steps

- [Working on projects](../guides/projects.md) for the day-to-day workflow in depth
- [Running scripts](../guides/scripts.md) for lifecycle hooks and named tasks
- [The lock file](../concepts/lockfile.md) for what `phast.lock` records and why
