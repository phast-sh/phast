---
description: "Install the phast binary, verify that it works, set up your shell, and update or uninstall it later."
---

# Installation

Phast ships as a single self-contained binary called `phast`.

## Install

On Linux and macOS:

```console
$ curl -fsSL https://raw.githubusercontent.com/phast-sh/phast/main/install.sh | sh
```

The script picks the build for your system (x86_64 or arm64; on Linux a static binary
that runs on any distribution), verifies it against the release's SHA-256 checksum, and
installs it to `~/.local/bin`. If that directory is not on your `PATH`, it prints the line
to add to your shell profile.

To choose a version or a location, set these before `sh`:

| Variable            | Default            | Meaning                         |
|---------------------|--------------------|---------------------------------|
| `PHAST_VERSION`     | the latest release | the release tag, e.g. `v0.1.0`  |
| `PHAST_INSTALL_DIR` | `~/.local/bin`     | where to put the `phast` binary |

```console
$ curl -fsSL https://raw.githubusercontent.com/phast-sh/phast/main/install.sh \
    | PHAST_VERSION=v0.1.0 PHAST_INSTALL_DIR=/usr/local/bin sh
```

## Build from source

On any platform with a Rust toolchain ([rustup](https://rustup.rs)):

```console
$ git clone https://github.com/phast-sh/phast.git
$ cd phast
$ cargo install --locked --path crates/phast
```

rustup installs the Rust version the repository pins, and `--locked` builds with the exact
dependency versions Phast is tested against. The binary goes to `~/.cargo/bin`, which
rustup adds to your `PATH`.

## Requirements

**PHP 8.0 or newer**, to use Phast: the generated `vendor/autoload.php` runs on any PHP
from 8.0 up, with no upper limit. Phast detects your PHP version and loaded extensions by
running the `php` binary, and uses it to execute lifecycle scripts. Resolution can even
run without PHP if you set `config.platform.php` in your manifest (see
[Configuration](../reference/configuration.md)).

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

If you installed with the script, update in place:

```console
$ phast cli update
```

It fetches the latest release, verifies its checksum, and atomically replaces the
running binary; if you are already current it does nothing. See
[`phast cli`](../reference/commands.md#phast-cli-versionupdate).

If you built from source, pull and reinstall:

```console
$ git -C phast pull
$ cargo install --locked --path phast/crates/phast --force
```

Uninstall a script install by deleting the binary from your install directory
(`~/.local/bin/phast` by default), or a source install with `cargo uninstall phast`.

## Next steps

Continue with [First steps](first-steps.md), or jump straight to
[Migrating from Composer](../guides/migrate-from-composer.md) if you have an existing
project.
