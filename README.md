<p align="center">
  <img src="docs/assets/logo/phast-ascii.svg" alt="Phast" width="340">
</p>

<p align="center"><em>PHP dependencies, at Rust speed.</em></p>

<p align="center">
  <a href="https://github.com/phast-sh/phast/actions/workflows/ci.yml"><img src="https://github.com/phast-sh/phast/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/phast-sh/phast/releases/latest"><img src="https://img.shields.io/github/v/release/phast-sh/phast?sort=semver&color=06b6d4" alt="Release"></a>
  <a href="https://docs.phast.sh"><img src="https://img.shields.io/badge/docs-phast-7fb6ae.svg?labelColor=0e7490" alt="Docs"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="License: MIT OR Apache-2.0"></a>
  <img src="https://img.shields.io/badge/platform-linux%20%7C%20macos-lightgrey.svg" alt="Platform">
  <a href="https://www.php.net/"><img src="https://img.shields.io/badge/PHP-8.0%2B-777BB4.svg?logo=php&logoColor=white" alt="PHP 8.0+"></a>
</p>

---

Phast is an extremely fast dependency manager for PHP, written in Rust. It is a drop-in
alternative to Composer: it resolves, downloads, and autoloads your dependencies from
Packagist and any Composer-compatible registry, while introducing a clean TOML-first
workflow built around `phast.toml` and `phast.lock`.

## Highlights

- **Blazing-fast installs.** Downloads and extraction run fully in parallel, so cold
  installs are quick and repeat installs are nearly instant.
- **Resolution in a blink.** A conflict-driven resolver makes short work of huge
  dependency graphs: real-world projects of 150+ packages resolve in well under a second.
- **Composer-compatible.** Works with Packagist, Private Packagist, and Satis, reads
  your existing `composer.json`, and generates a `vendor/autoload.php` your application
  uses exactly as before (including the `\Composer\InstalledVersions` runtime API).
- **TOML-first.** A readable `phast.toml` manifest and a reproducible, merge-friendly
  `phast.lock` with stable ordering that keeps diffs small and conflicts rare.
- **Scripts runtime.** Run named scripts with `phast run`, and Composer lifecycle
  events (`pre-install-cmd`, `post-update-cmd`, …) fire automatically, with a
  `\Composer\Script\Event` shim so your existing handlers just work.
- **Errors that actually help.** When resolution fails, Phast tells you *why*: a
  readable account of the conflicting constraints and which package pulled them in, not a
  wall of output.
- **Workspaces.** First-class monorepos with shared constraints, one lock file, and one
  `vendor/` directory: no path-repository wiring.
- **Secure by default.** Lockfile checksums, a `minimum-package-age` supply-chain
  guard, and a built-in `audit` against Packagist and OSV advisories.

## Installation

**Linux and macOS** (prebuilt binary):

```sh
curl -fsSL https://raw.githubusercontent.com/phast-sh/phast/main/install.sh | sh
```

**From source** (any platform with a Rust toolchain):

```sh
git clone https://github.com/phast-sh/phast.git
cd phast
cargo install --path crates/phast
```

Verify with `phast cli version`. Update later with `phast cli update`.

## Quick start

```sh
# In an existing Composer project: convert composer.json to phast.toml
$ phast migrate

# Resolve, download, and generate the autoloader
$ phast install

# Add and remove dependencies
$ phast add monolog/monolog:^3.0
$ phast remove monolog/monolog

# Inspect and maintain
$ phast show                  # list installed packages
$ phast why monolog/monolog   # show what requires a package
$ phast outdated              # list packages with newer versions available
$ phast audit                 # scan the lock for security advisories
```

See `phast help` for the full command list.

## Sneak peek

**See exactly where the time goes.** Add `-v` and Phast reports every phase, so you can
watch how little of it there is:

```console
$ phast install -v
Resolved 151 packages in 0.14s
Locked 151 packages in 0.02s
Installed 151 packages in 2.41s
Autoloaded 151 packages in 0.05s
Finished in 2.62s (151 packages)
```

**When something doesn't fit, you'll know why.** A resolution conflict is explained, not
dumped: which package wants what, and who pulled it in.

```console
$ phast install
Error: [dependency conflict] Phast could not find compatible versions for your requirements
 ╰─▶ monolog/monolog has incompatible constraints:
       · your project requires monolog/monolog[>=3.0, <4.0]
       · acme/reporting[>=1.0, <2.0] requires monolog/monolog[>=2.0, <3.0]
```

**Monorepos without the wiring.** A `[workspace]` section resolves all members together
into a single shared lock file and one `vendor/`, with constraints from
`[workspace.dependencies]`:

```console
$ phast workspace
acme/platform
  acme/core   1.4.0   packages/core
  acme/api    1.4.0   packages/api   (current)
  acme/web    1.4.0   packages/web
```

**Stay current, safely.** `phast outdated` flags newer releases, `phast audit` scans the
lock against security advisories, and `phast cli update` upgrades Phast itself in place.

## Documentation

Phast's documentation is available at [docs.phast.sh](https://docs.phast.sh).

The command-line reference is also available locally with `phast help`.

## Status

Phast is in active development. The current release is `0.1.0` **Ai-Petri**; manifest and
lock formats may still change before `1.0`. Feedback and issues are very welcome.

## Security

Please report any sensitive issue privately, either through
[GitHub](https://github.com/phast-sh/phast/security/advisories/new) or
[security@phast.sh](mailto:security@phast.sh); see [SECURITY.md](SECURITY.md)
for the full policy.

## License

Phast is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Third-party attributions are listed in [NOTICE](NOTICE) and
[LICENSE-THIRD-PARTY](LICENSE-THIRD-PARTY).
