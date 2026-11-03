---
description: "Reference for the [config] section of phast.toml: minimum-stability, minimum-package-age, secure-http, platform checks and the other settings."
---

# Configuration

Reference for the `[config]` section of `phast.toml`. All keys are optional.

```toml
[config]
minimum-stability      = "stable"
prefer-stable          = true
sort-packages          = true
minimum-package-age    = "7d"
secure-http            = true
classmap-authoritative = false
platform-check         = "php"

[config.platform]
php      = "8.2"
ext-intl = "1.0"
```

## `minimum-stability`

The lowest release stability the resolver may select. Accepted values, least to most
stable: `dev`, `alpha`, `beta`, `rc`, `stable`. Default: `stable`.

Versions below the threshold are invisible to the resolver, as if they were never
published. Setting `"beta"` allows beta and RC pre-releases everywhere.

To relax (or tighten) the floor for a single package instead of the whole project, put a
trailing `@stability` flag on its constraint: `"acme/sdk" = "^2.0@dev"`. See
[Per-package stability](../concepts/resolution.md#per-package-stability).

## `prefer-stable`

Boolean, default `false`. Accepted for Composer compatibility; version selection
currently always picks the newest version allowed by `minimum-stability` and your
constraints.

## `sort-packages`

Boolean, default `false`. When `true`, every command that writes `phast.toml` keeps
`[require]` and `[require-dev]` sorted: platform packages (`php`, `ext-*`) first, then
alphabetically. `phast fmt` applies the same order.

## `minimum-package-age`

A duration string: a number followed by `h` (hours), `d` (days), or `w` (weeks), e.g.
`"24h"`, `"7d"`, `"2w"`. Unset by default.

When set, versions published more recently than the threshold are excluded from
resolution entirely. This is a supply-chain guard; behavior details and recommended
values are covered in [Security](../guides/security.md#guarding-against-fresh-supply-chain-attacks).
Versions without a publish timestamp pass through, and versions already in `phast.lock`
are not re-checked.

There is no upper limit, and no warning. `"365w"` is accepted and holds back every
release from the last seven years. The usual range is `"24h"` to `"2w"`.

## `secure-http`

Boolean, default `true`. When enabled, Phast refuses to make registry or download
requests over plaintext `http://` and never attaches credentials to a non-HTTPS request,
so a mistyped repository URL or a man-in-the-middle cannot capture your tokens. Loopback
hosts (`localhost`, `127.0.0.0/8`, `::1`) are exempt, since that traffic never leaves the
machine.

Set it to `false` only for a trusted internal registry served over plain HTTP:

```toml
[config]
secure-http = false
```

This mirrors Composer's `secure-http`, which is also on by default. Behavior details are
covered in [Security](../guides/security.md#transport-security).

## `classmap-authoritative`

Boolean, default `false`. When `true`, the generated autoloader trusts the classmap
exclusively: a class absent from the map is treated as nonexistent and the PSR
filesystem fallback is skipped entirely. This removes filesystem probing from every
class-lookup miss, at a cost: classes generated at runtime (Doctrine proxies, code
generators) stop loading. Intended for immutable production trees.

The `-a`/`--classmap-authoritative` flag on `phast install` and `phast autoload`
enables the same mode for a single run without changing the manifest.

## `platform-check`

String, default `"php"`. Controls whether `vendor/phast/platform_check.php` is
generated and how much of the platform it verifies when the autoloader boots. The
requirements are aggregated from the root manifest and the entire locked tree, so the
guard reflects what the dependency set actually demands.

| Value   | Meaning                                              |
|---------|------------------------------------------------------|
| `"php"` | Verify the PHP version only (default)                |
| `"all"` | Verify the PHP version and every required extension  |
| `"off"` | Do not generate the guard                            |

When a requirement is not met, the guard throws a `RuntimeException` at bootstrap with
a message naming each unmet requirement, instead of letting the application fail later
in an arbitrary place.

## `[config.platform]`

Overrides the runtime platform used during resolution. Without overrides, Phast detects
the local `php` binary's version and loaded extensions.

| Key       | Example   | Meaning                                          |
|-----------|-----------|--------------------------------------------------|
| `php`     | `"8.2"`   | Resolve as if this PHP version were installed    |
| `ext-*`   | `"1.0"`   | Declare an extension as present at this version  |

Overrides do not change the PHP that actually runs your code; they only pin what the
resolver believes, which makes resolution reproducible across machines and allows
resolving for a deployment target that differs from the development machine (or on a
machine without PHP at all, if `php` is set explicitly).

`phast init` and `phast migrate` write `php` here automatically.

## `[config.allow-plugins]`

Accepted for compatibility with migrated Composer manifests
(`"vendor/plugin" = true|false`) but carries no behavior: Phast never runs plugin
code today, and this key is not how plugins will be enabled. Phast's planned plugin
system is opt-in by declaration: a plugin only runs when explicitly listed in a
dedicated `[plugins]` section, so the declaration itself is the consent and no
separate allowlist is needed. Until that ships, the key is inert.

## Related settings outside `[config]`

- Registries and package routing: `[[repositories]]` and `[sources]`, see
  [Registries and authentication](../concepts/registries.md).
- Credentials: `auth.toml` / `auth.json` files, never in the manifest.
- Cache location: the `--cache-dir` flag or `PHAST_CACHE_DIR`, see
  [Environment variables](environment.md).
