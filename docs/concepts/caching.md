---
description: "Phast's machine-wide cache of registry metadata and package archives: where it lives, how to inspect and clear it, and how to reuse it in CI."
---

# Caching

Phast keeps a machine-wide disk cache with two layers:

| Layer       | Contents                                       | Invalidation                       |
|-------------|------------------------------------------------|------------------------------------|
| `metadata/` | Registry responses, isolated per registry host | Revalidated with ETags on each use |
| `archives/` | Downloaded package archives, content-addressed | Never; content is immutable        |

Metadata revalidation means a warm cache costs one cheap conditional request per
package (`304 Not Modified`) instead of a re-download. Archives are keyed by their
checksum, so the same package version is stored once and shared across every project on
the machine; a cached archive is reused without re-downloading.

Every cached archive is checksum-verified before it is extracted, not just on the
download that first populated it. A corrupted or tampered cache entry therefore cannot
reach `vendor/`: Phast deletes it and re-downloads a clean copy. This makes a shared or
CI-restored cache safe to trust without re-fetching everything to confirm it. See
[Archive integrity](../guides/security.md#archive-integrity).

## Location

| Platform | Default                  |
|----------|--------------------------|
| Linux    | `~/.cache/phast`         |
| macOS    | `~/Library/Caches/phast` |
| Windows  | `%LOCALAPPDATA%\phast`   |

Override it with the global `--cache-dir` flag or the `PHAST_CACHE_DIR` environment
variable (the flag wins). Every command respects the override, including `phast cache`
itself.

## Inspecting and clearing

```console
$ phast cache size

  metadata   12 MB   ~/Library/Caches/phast/metadata
  archives   38 MB   ~/Library/Caches/phast/archives
  ─────────────────
  total      50 MB

$ phast cache clear
Cleared cache at ~/Library/Caches/phast (50 MB freed).
```

Both subcommands take `--metadata-only` (`-m`) or `--archives-only` (`-a`) to target one
layer. Clearing metadata forces fresh registry lookups on the next install; clearing
archives forces re-downloads. Neither affects correctness, only speed.

## Using the cache in CI

Point the cache at a path your CI system can persist and restore it between runs:

```yaml
# GitHub Actions example
- name: Restore Phast cache
  uses: actions/cache@v4
  with:
    path: .phast-cache
    key: phast-${{ hashFiles('phast.lock') }}
    restore-keys: phast-

- name: Install dependencies
  run: phast install --cache-dir .phast-cache
```

Or set it once for every step:

```yaml
env:
  PHAST_CACHE_DIR: .phast-cache
```

Because archives are content-addressed, the cache is safe to share across jobs and
branches: a lock change invalidates the key, but unchanged packages are still reused
through `restore-keys`, so even a "miss" only downloads what actually changed.
