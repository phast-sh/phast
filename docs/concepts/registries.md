---
description: "Use Packagist, private Composer registries, git repositories and local paths with Phast, and keep credentials in auth files, out of phast.toml."
---

# Registries and authentication

By default Phast resolves against Packagist (`repo.packagist.org`). Projects with
private packages add registries in the manifest; credentials live in auth files, never
in `phast.toml`.

## Adding registries

```toml
[[repositories]]
name = "nova"
type = "composer"
url  = "https://nova.laravel.com"

[[repositories]]
name = "internal"
type = "composer"
url  = "https://satis.example.com"
```

Any Composer-compatible registry works: Private Packagist, Satis, GitLab/GitHub package
registries, or anything else speaking the Composer repository protocol. Phast prefers
the modern v2 metadata API and falls back automatically to the v1 format used by older
Satis instances, so legacy internal registries work without changes.

Packagist itself stays available as the implicit fallback. To redirect it to a mirror
or disable it entirely (fully private projects):

```toml
[[repositories]]
type = "packagist"
url  = "https://packagist.internal.example.com"   # mirror

[[repositories]]
type = "packagist"                                # no url: Packagist disabled
```

Single-package sources (a local path, a git repository, or an inline self-describing
package) are attached per package in `[sources]` rather than as `[[repositories]]`
entries; see [Pinning and per-package sources](#pinning-and-per-package-sources-with-sources)
below. Local `path` and `vcs` (git) repositories serving a single package can also be
written as `[[repositories]]` entries, covered in
[Local path repositories](#local-path-repositories) and
[VCS (git) repositories](#vcs-git-repositories).

!!! note
    Registry URLs must be `https://` by default: plaintext `http://` is rejected so
    credentials and metadata cannot travel unencrypted (`localhost` is exempt). For a
    trusted internal registry on plain HTTP, set `secure-http = false` in `[config]`; see
    [Security](../guides/security.md#transport-security).

For one-off testing there is also a command-line override that bypasses the manifest
list entirely: `phast install --registry-url https://...`.

## Local path repositories

A `path` repository installs a package straight from a directory on disk instead of a
registry, for developing two packages side by side, or wiring a small monorepo without
the full [workspaces](../guides/workspaces.md) feature:

```toml
# myapp/phast.toml
[require]
"acme/greeter" = "^1.0"

[[repositories]]
type = "path"
url  = "packages/greeter"   # relative to this manifest
```

The target directory holds its own manifest (`phast.toml` or `composer.json`) declaring
the package `name` and a `version`:

```toml
# packages/greeter/phast.toml
[package]
name    = "acme/greeter"
version = "1.0.0"
```

On `phast install`, Phast reads the local manifest, matches it to the requirement by
name, and symlinks it into `vendor/acme/greeter` pointing at the source directory. Edits
to the source are visible immediately, with no reinstall.

Notes and current limits:

- The package **must declare a `version`**, and it has to satisfy the requiring
  constraint (`^1.0` above). Phast does not invent a version for an unversioned package.
- The package name routes only to its path repository, never to Packagist, so a local
  package can never be shadowed by a public one of the same name.
- Materialization is a symlink on macOS and Linux; Windows is not supported yet.
  Copying instead of linking is not supported either, so `symlink = false` is
  **rejected when the manifest loads**. Writing `symlink = true` is allowed and 
  simply restates the default.

## VCS (git) repositories

A `vcs` repository installs a package straight from a git host, reading the
repository's tags for the available versions and pulling the matching commit's
archive, instead of going through a registry. Useful for a fork, a private
package published to no registry, or a package you want to track from source:

```toml
[require]
"psr/log" = "^3.0"

[[repositories]]
type = "vcs"
url  = "https://github.com/php-fig/log"
```

Phast reads the repository's manifest (`composer.json`, falling back to
`phast.toml`) to learn the package `name`, lists its git tags as the available
versions, and resolves the requiring constraint against them. The selected
version installs from the host's zipball for that tag, extracted into `vendor/`
like any other package.

Notes and current limits:

- **GitHub only** for now (`github.com`). The host layer is generic; GitLab and
  others are planned. A repository on an unsupported host is warned about and
  skipped.
- Versions come from **git tags** (a leading `v` is stripped, so `v1.2.0` and
  `1.2.0` are the same version); branches are available as `dev-{branch}` versions.
- **Branch aliases** are honored: if the repository's own manifest declares
  `extra.branch-alias` (e.g. `{ "dev-main": "1.2.x-dev" }`), that branch also
  presents as the aliased dev version, so a numeric require like `^1.2` can resolve
  to the branch. Aliases are dev-stability, so they only apply when
  `minimum-stability = "dev"`; the package then locks under its branch
  (`dev-main`) at that commit.
- Like a path repository, the package self-pins to its VCS repository and is
  never resolved from Packagist, so it cannot be shadowed by a public package of
  the same name. Because of that, the entry above needs no `[sources]` line.
- **`git` is an accepted spelling of `type = "vcs"`** and behaves identically: the
  host is detected from the URL either way. **Subversion is not supported**;
  `type = "svn"` is rejected when the manifest loads.
- Authentication uses the GitHub OAuth token from your auth files (`[github-token]`
  keyed by `github.com`); see [Authentication](#authentication). Public
  repositories work unauthenticated, but that shares GitHub's 60 requests/hour
  per-IP limit, so a token is recommended.

Give the entry a `name` if you want to point at it explicitly from `[sources]`:

```toml
[[repositories]]
name = "fig-log"
type = "vcs"
url  = "https://github.com/php-fig/log"

[sources]
"psr/log" = { repository = "fig-log" }
```

For a repository serving one package this is documentation rather than necessity,
since the package already self-pins. It becomes load-bearing when one repository
provides several packages, which is why the name exists.

## Pinning and per-package sources with `[sources]`

`[sources]` attaches a single package to exactly one source. It does two jobs: it pins
a package to a registry (dependency-confusion control), and it declares single-package
git, path, and inline sources without an `[[repositories]]` entry. Every value is an
inline table keyed off the package name, so the source is explicit per package:

```toml
[sources]
"laravel/nova"   = { repository = "nova" }                                                             # a named endpoint
"acme/io"        = { url = "https://github.com/acme/io", branch = "main" }                             # git source
"acme/tools"     = { url = "https://github.com/acme/tools", tag = "v2.3.1" }                           # git at a tag
"acme/contracts" = { path = "packages/contracts", version = "3.7.x-dev" }                              # local path
"acme/cmd"       = { url = "https://github.com/acme/cmd", version = "1.4.0", reference = "abc1234" }   # inline package
```

A value is one of four shapes, classified by which fields are set:

- **`{ repository = "name" }`**: the package is
  fetched *only* from that named `[[repositories]]` endpoint. Registry pins are the
  dependency-confusion control: a pinned package cannot be silently shadowed by a
  same-named package on a public registry, your private registry never learns which
  public packages you use, and Packagist never receives your private names. With full
  pinning, resolution costs the same number of requests as a single-registry setup.
- **`{ url, branch | tag | reference }`**: a single-package git source, Phast reading the
  repository's own manifest for the package metadata. Unlike a `vcs`
  `[[repositories]]` entry, which offers every tag and branch and lets the resolver
  choose, this **pins**: the ref you name is the decision, and it is the only version
  offered. See [Pinning a single git package](#pinning-a-single-git-package) below.
- **`{ path, symlink?, version? }`**: a local package on disk, materialized by symlink,
  with `version` supplying the package version when its own manifest does not.
  `symlink` may only be `true`; see [Local path repositories](#local-path-repositories).
- **`{ url, version, reference? }`**: an inline, self-describing package. The spec *is*
  the package (no metadata fetch); Phast installs it from a synthesized archive of the
  git source at that reference. This is how a project vendors a dependency that is on no
  registry.

Every package named in `[sources]` self-pins to its source and is excluded from the
registry fallback. Packages not listed use the remaining registries in declaration
order, with Packagist last.

### Pinning a single git package

For one package from one git repository, a `[sources]` entry is the whole configuration:

```toml
[require]
"acme/design-system" = "*"

[sources]
"acme/design-system" = { url = "https://github.com/acme/design-system", branch = "main" }
```

The ref decides what you get, so exactly one version is offered and your constraint is
checked against it (which is why `"*"` is the usual choice):

| Spec                            | Resolves to                                      |
|---------------------------------|--------------------------------------------------|
| `{ url, branch = "main" }`      | `dev-main`                                       |
| `{ url, tag = "v2.3.1" }`       | that tag's version                               |
| `{ url, reference = "abc123" }` | the version its manifest declares at that commit |
| `{ url }`                       | the repository's default branch, as `dev-*`      |

Whichever you name, the lock records the **commit** it resolved to, so a moving branch
still produces a reproducible install.

Because the package name comes from the `[sources]` key, Phast checks the repository
agrees with it and fails if not, rather than installing something under a name you did
not ask for:

```
[sources] 'acme/design-system' pins https://github.com/acme/other, but its manifest declares 'acme/other'
```

Use a `vcs` `[[repositories]]` entry instead when you want the resolver to *choose*
among the repository's tags; use this when you want to name the ref yourself.

## Authentication

Credentials are looked up per hostname, from these sources in priority order:

| Priority | Source                                 | Notes                              |
|----------|----------------------------------------|------------------------------------|
| 1        | `./auth.toml`                          | Project-local, Phast-native format |
| 2        | `./auth.json`                          | Project-local, Composer-compatible |
| 3        | `~/.phast/auth.toml`                   | Global Phast credentials           |
| 4        | `~/.composer/auth.json`                | Global Composer credentials        |
| 5        | Manifest `[config]` inline credentials | Lowest priority                    |

Higher sources win per hostname, so your local `auth.toml` always overrides global
files and anything committed with the project. Existing Composer setups keep working
with no changes thanks to entries 2 and 4.

`auth.toml` format:

```toml
[http-basic]
"nova.laravel.com" = { username = "you@example.com", password = "secret" }

[bearer]
"satis.example.com" = "my-access-token"

[github-token]
"github.com" = "ghp_yourTokenHere"
```

Bearer tokens, HTTP basic auth, GitHub OAuth tokens (`github-token`), and GitLab tokens
(`gitlab-token`) are supported. The matched credential is applied to every request to
that host, both metadata fetches and archive downloads, so private dist URLs work with
no extra configuration.

!!! warning
    Keep `auth.toml` and `auth.json` out of version control. `phast migrate` extracts
    inline credentials from a `composer.json` into `auth.toml` for exactly this reason,
    and reminds you to gitignore it.

## Cache isolation

Metadata is cached per registry host, so responses from different registries can never
shadow each other, even for identical package names. See [Caching](caching.md).
