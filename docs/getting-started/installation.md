---
description: "Install the phast binary, verify that it works, set up your shell, and update or uninstall it later."
---

# Installation

Phast ships as a single self-contained binary called `phast`.

!!! note
    Phast is in active development. Prebuilt binaries, an installer script, and a
    Homebrew formula are planned; for now Phast is built from source.

## Requirements

- **Rust toolchain** (to build): install via [rustup](https://rustup.rs) if you do not
  have it.
- **PHP 8.0 or newer** (to use): the generated `vendor/autoload.php` runs on any PHP from
  8.0 up, with no upper limit. Phast detects your PHP version and loaded extensions by
  running the `php` binary, and uses it to execute lifecycle scripts. PHP does not need to be
  installed to *build* Phast, and resolution can even run without PHP if you set
  `config.platform.php` in your manifest (see [Configuration](../reference/configuration.md)).

## Build from source

```console
$ git clone https://github.com/phast-sh/phast.git
$ cd phast
$ cargo install --path crates/phast
```

`cargo install` places the binary in `~/.cargo/bin`, which rustup adds to your `PATH` by
default.

## Verify the installation

```console
$ phast --version
```

Then check that Phast sees your PHP:

```console
$ php --version
```

If `php` is not on your `PATH` or you want Phast to use a specific interpreter (for
example, one from a version manager), point the `PHAST_PHP_BIN` environment variable at
it:

```console
$ export PHAST_PHP_BIN=/opt/homebrew/opt/php@8.3/bin/php
```

## Shell environment

Phast reads its settings, like `PHAST_PHP_BIN` or `PHAST_CACHE_DIR`, from your shell
environment. See [Environment variables](../reference/environment.md) for the full list, and how to set them
per project.

## Updating and uninstalling

If you built from source, pull and reinstall:

```console
$ git -C phast pull
$ cargo install --path phast/crates/phast --force
```

If you installed a release binary (Linux or macOS), update in place with:

```console
$ phast cli update
```

It fetches the latest release, verifies its checksum, and atomically replaces the
running binary; if you are already current it does nothing. See
[`phast cli`](../reference/commands.md#phast-cli-versionupdate).

Uninstall a source install with `cargo uninstall phast`, or a binary install by
deleting it from your install directory (`~/.local/bin/phast` by default).

## Next steps

Continue with [First steps](first-steps.md), or jump straight to
[Migrating from Composer](../guides/migrate-from-composer.md) if you have an existing
project.
