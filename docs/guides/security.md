---
description: "How Phast secures the supply chain: HTTPS-only registries, checksum-verified archives, a minimum package age, and phast audit for known advisories."
---

# Security

Phast's defenses span the whole supply chain. On the way in, every registry request uses
verified transport and every downloaded archive is checked against its published
checksum. During resolution, `minimum-package-age` keeps *freshly published* (and
therefore unvetted) releases out of selection. After the fact, `phast audit` finds
*known* vulnerabilities in what you already locked.

## Transport security

By default Phast refuses to talk to a registry over plaintext `http://`, and never
attaches credentials (bearer tokens, basic-auth passwords) to a non-HTTPS request. A
mistyped repository URL or a man-in-the-middle therefore cannot capture your tokens, and
metadata cannot be silently swapped in transit.

Loopback hosts (`localhost`, `127.0.0.0/8`, `::1`) are exempt, since that traffic never
leaves the machine. To allow a trusted internal registry served over plain HTTP, opt out
in `[config]`:

```toml
[config]
secure-http = false
```

With `secure-http = false`, plaintext URLs are permitted and credentials are sent over
them. Only do this on a network you trust end to end. This mirrors Composer's
`secure-http`, which is also on by default.

## Archive integrity

Packagist publishes a SHA-1 `shasum` for every package archive. Phast verifies the bytes
against it before extracting them into `vendor/`, so a corrupted download, a tampered
mirror, or a damaged cache entry can never reach your project. Verification runs on
**every** extraction, including cache hits, not just fresh downloads. On a mismatch Phast
deletes the offending cache entry and fails the install; the next run re-downloads a
clean copy.

If a registry publishes *no* checksum for a package, it is installed without
verification, and Phast does not yet say which packages those were. Packagist dist
entries almost always carry a checksum; a missing one most often means a custom or
misconfigured registry.

## Checking for known vulnerabilities

```console
$ phast audit [--provider=packagist]

Phast found 2 security advisories in your locked dependencies.

  guzzlehttp/guzzle 7.4.0
    CVE-2022-31091  [HIGH]    Change in port should be considered a change in origin
                              affected  >=7.0,<7.4.5
                              reported  2022-06-20
                              https://github.com/advisories/...

Run `phast update` to attempt resolution.
```

`--provider` selects the advisory database, and defaults to `packagist`:

| Provider    | Source                                                             |
|-------------|--------------------------------------------------------------------|
| `packagist` | Packagist's security advisory API, the canonical source for PHP    |
| `osv`       | Google's OSV database (aggregates GitHub Advisory Database et al.) |

The two databases overlap but are not identical; for belt-and-braces coverage run both.
Advisories are matched against the exact versions in `phast.lock`, never against
constraints, and the command exits with code 1 when anything is found, so a plain
`phast audit` works as a CI gate.

Flags:

| Flag                 | Effect                                                      |
|----------------------|-------------------------------------------------------------|
| `--no-dev`           | Audit production packages only                              |
| `--since YYYY-MM-DD` | Only report advisories published on or after this date      |
| `--ignore <ID>`      | Suppress a specific advisory (repeatable, case-insensitive) |
| `--format json`      | Machine-readable output for dashboards and tooling          |

`--since` is for teams that have triaged their backlog and want CI to alert only on new
advisories. `--ignore CVE-2023-12345` is for individually accepted risks; ignored
advisories do not count toward the exit code, so a gate passes when only acknowledged
findings remain. Both are deliberately command-line flags rather than manifest settings:
security policy stays out of `phast.toml` and visible in your CI configuration.

When a date or constraint in the advisory data cannot be interpreted, Phast errs on the
side of reporting: a finding is never silently dropped because of malformed metadata.

In a workspace, `audit` always covers the entire shared lock regardless of where it is
invoked, because everything in the lock is installed in the shared `vendor/`.

## Guarding against fresh supply-chain attacks

Most registry supply-chain attacks follow the same timeline: a malicious version is
published, installed by victims within hours, and removed or flagged within days. An
analysis of prominent attacks found most were only exploitable within their first week.

`minimum-package-age` closes that window. Set it in `[config]`:

```toml
[config]
minimum-package-age = "7d"   # or "24h", "2w"
```

During resolution, any version published more recently than the threshold is treated as
if it does not exist. Your installs simply stay one step behind the bleeding edge, which
for almost every project is free insurance.

Recommended values:

- `"24h"`: catches same-day typosquatting and malware pushes with minimal lag.
- `"7d"`: the security-conscious sweet spot; gives researchers and registries time to
  flag malicious releases.

Behavior details:

- The feature is opt-in and has no default; teams that need same-day releases (internal
  registries, hotfix pipelines) are unaffected.
- It applies to dev and production dependencies alike; build tooling is an equally
  valid attack vector.
- Versions already recorded in `phast.lock` are not re-checked: the gate applies when a
  version is *selected*, not on every install.
- Packages without a publish timestamp (some very old releases) pass through; a freshly
  uploaded malicious version always carries one.
- If every version satisfying a constraint is too new, resolution fails with an error
  saying so, rather than silently picking something else:

  ```console
  Error: 'vendor/alpha' was not resolved: no configured registry offers a version of it.
   hint: vendor/alpha 2.1 was published 2026-10-30, inside minimum-package-age, so it is not offered until 2026-11-06
  ```

## Scripts in a repository you have not reviewed

`phast install`, `phast update` and `phast autoload` run the hooks the project declares in
its own `[scripts]` (see [Running scripts](scripts.md#lifecycle-hooks)). In a repository
you have just cloned, those are commands someone else wrote, and they run with your
permissions the moment you install. Only the project's own manifest declares hooks: the
packages Phast installs into `vendor/` run nothing during an install.

To look before you run anything, preview the install. A dry run resolves and lists what
would be installed, runs no hooks and writes nothing:

```console
$ phast install --dry-run
```

Then read the `[scripts]` in `phast.toml`, and install without them until you trust them:

```console
$ phast install --no-scripts
```

`phast add` and `phast remove` run no hooks when they install.

## A sensible CI setup

```yaml
- run: phast install
- run: phast audit                               # gate on known CVEs
- run: phast outdated || true                    # visibility, non-blocking
```

Combined with `minimum-package-age` in the manifest, this covers both directions:
nothing known-bad gets through, and nothing too-new-to-judge gets in.

For pull requests from forks, where `phast.toml` itself comes from the contributor, install
with `--no-scripts` so a changed hook cannot run in your CI:

```yaml
- run: phast install --no-scripts
```

## Reporting a vulnerability in Phast

Everything above is about the packages Phast installs. If you find a vulnerability in
Phast itself, please report it privately, through [GitHub private vulnerability reporting](https://github.com/phast-sh/phast/security/advisories/new)
or [security@phast.sh](mailto:security@phast.sh). The full policy, including scope and what to include, is
in the repository's [SECURITY.md](https://github.com/phast-sh/phast/blob/main/SECURITY.md).
