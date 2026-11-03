---
description: "Reference for every Phast command and its options, from phast install and phast add to phast audit, phast why and phast migrate."
---

# Commands

Reference for every Phast command. `phast help <command>` shows the same information in
the terminal.

## Global options

Available on every command:

| Option              | Description                                                       |
|---------------------|-------------------------------------------------------------------|
| `-q`, `--quiet`     | Suppress all output except errors                                 |
| `-v`, `--verbose`   | Increase verbosity: `-v` phase timings, `-vv` per-package detail  |
| `--cache-dir <dir>` | Override the cache directory (env: `PHAST_CACHE_DIR`)             |
| `-V`, `--version`   | Print the Phast version                                           |
| `-h`, `--help`      | Print help                                                        |

Unless noted otherwise, commands exit `0` on success and non-zero on failure.

## Project options

Two more options recur throughout, and mean the same thing wherever they appear:

| Option                | Description                                                            |
|-----------------------|------------------------------------------------------------------------|
| `--project-dir <dir>` | Directory to operate in; defaults to the current directory             |
| `--member <path>`     | Workspace member to target, by its path relative to the workspace root |

`--member` names a member by *path*, exactly as `phast workspace` prints it
(`packages/logger`), not by package name. It requires an active workspace; outside one
it fails with:

```
--member requires a workspace. Add a [workspace] section to phast.toml first
```

A path matching no member fails with `'packages/loger' is not a workspace member`, so a
typo is reported rather than quietly resolved to something nearby.

Standing inside a member's directory means the same as naming it. Every command that
takes `--member` scopes itself to the member containing the working directory when the
flag is absent, so passing the flag and running from the right place are interchangeable.

Not every command takes both. `lock`, `autoload`, `audit`, and `migrate` take
`--project-dir` but no `--member`: the lock, the installed tree, and an advisory report
each cover the project as a whole. `init`, `cache`, and `cli` take neither; `init` names
its directory as a positional argument, and the other two operate outside any project.

---

## `phast init [path]`

Scaffold a new package: package name, type, description, PHP version, and optional
`src/`/`tests/` directories with PSR-4 mappings derived from the package name. Writes
`phast.toml` into `path` (created if needed; defaults to the current directory). Fails
if a `phast.toml` already exists there.

Anything not given as an option is prompted for. A prompt is skipped when its option is
supplied, so `phast init --name acme/blog` still asks for the rest. Prompting is skipped
altogether with `--yes`, or whenever stdin is not a terminal, which is what makes `init`
usable from a script, a Dockerfile, or a CI job.

| Option                    | Description                                                       |
|---------------------------|-------------------------------------------------------------------|
| `--name <vendor/package>` | Package name. Defaults to `$USER/<directory>`                     |
| `--type <type>`           | `library`, `project`, or `phast-plugin`. Defaults to `library`    |
| `--description <text>`    | Package description. Omitted from the manifest when not given     |
| `--php <constraint>`      | PHP version constraint, e.g. `^8.4`. Defaults to the detected PHP |
| `--no-src`                | Do not create `src/`                                              |
| `--no-tests`              | Do not create `tests/`                                            |
| `-y`, `--yes`             | Take the defaults for anything not given, without prompting       |

Package names are normalized to lowercase, as everywhere else in Phast.

The PHP constraint is the one field with no fallback. When no runtime can be detected
and `--php` was not given, a run that cannot prompt fails and writes nothing rather than
guessing a version that would surface as a resolution failure later.

## `phast install`

Install dependencies. Resolves only when `phast.lock` is missing or out of sync with
`phast.toml`; otherwise installs straight from the lock. Downloads missing packages,
generates `vendor/autoload.php` and `vendor/bin/` stubs, and fires install lifecycle
scripts.

| Option                           | Description                                                              |
|----------------------------------|--------------------------------------------------------------------------|
| `--no-dev`                       | Skip installing `[require-dev]` packages; the lock still records them    |
| `--dry-run`                      | Show what would be installed; write nothing and run no hooks             |
| `--no-scripts`                   | Skip all lifecycle script hooks                                          |
| `-a`, `--classmap-authoritative` | Trust the classmap exclusively; drop the PSR filesystem fallback         |
| `--registry-url <url>`           | Resolve against this single registry, ignoring the manifest repositories |
| `--project-dir <dir>`            | Project directory (default: current directory)                           |
| `--member <path>`                | Only download one workspace member's dependency closure                  |

## `phast update [packages...]`

Re-resolve to the newest versions allowed by `phast.toml`. With package arguments, all
*other* locked packages are pinned to their current versions, so only the named ones
(and packages only they need) can change; naming a package not present in the lock is
an error. Re-downloads changed packages and regenerates the autoloader.

| Option                 | Description                                                           |
|------------------------|-----------------------------------------------------------------------|
| `--no-dev`             | Skip installing `[require-dev]` packages; the lock still records them |
| `--dry-run`            | Show what would change; write nothing and run no hooks                |
| `--no-scripts`         | Skip all lifecycle script hooks                                       |
| `--registry-url <url>` | Resolve against this single registry                                  |
| `--project-dir <dir>`  | Project directory (default: current directory)                        |
| `--member <path>`      | Take the package list from one workspace member                       |

In a workspace, `--member` (or a working directory inside one) supplies the package list
when none was named: `phast update --member packages/api` updates what that member
requires, and nothing else. Naming packages explicitly wins over the member, being the
more specific answer. Either way the lock stays workspace-wide, so this chooses which
packages may move, not which part of the lock is rewritten.

## `phast add <packages...>`

Add dependencies and install. Each argument is `vendor/package` or
`vendor/package:constraint`. Updates the constraint in place when the package is already
present; fails if it is present in the opposite section.

Given no constraint, Phast resolves the whole project with the package required and
records `^major.minor` of the version that came out, so the constraint written is one that
resolves against everything else you depend on. Platform packages (`php`, `ext-*`) are
recorded as `*`: they describe the environment, not a version range to track. A package
that nothing installs, because another one carries it through `[provide]` or `[replace]`,
is recorded as `*` too: there is no version to derive a range from, and an interface
states what must be available rather than a range to follow.

| Option                 | Description                                        |
|------------------------|----------------------------------------------------|
| `--dev`                | Add to `[require-dev]` instead of `[require]`      |
| `--no-install`         | Update `phast.toml` only; skip the install         |
| `--registry-url <url>` | Resolve against this single registry               |
| `--project-dir <dir>`  | Project directory (default: current directory)     |
| `--member <path>`      | Target a workspace member's manifest               |

### Inside a workspace

A workspace root declares no dependencies of its own, so `add` at the root is refused:

```
a workspace root has no dependencies of its own. Target a member with --member <path>
```

Within a member, what gets recorded depends on what the workspace already answers for the
package:

- **The root pins it in `[workspace.dependencies]`.** The member records
  `{ workspace = true }` rather than spelling out a constraint the root owns. Giving an
  explicit constraint that disagrees with the pin is refused, not narrowed: only the root
  decides what every member gets. Dropping the constraint inherits it; changing it means
  changing the pin at the root.
- **A sibling member declares it.** The constraint is derived as `^major.minor` from that
  member's `[package].version`, with no registry involved: the package is satisfied from
  the source tree, so there is nothing to look up. An explicit constraint wins, being the
  more specific answer. A sibling that declares no version is an error rather than a
  guess, naming the manifest to fix.

`--no-install` skips the download, not the network: choosing a version requires a
registry. When there is nothing to choose, nothing is asked. That covers a constraint
given on the command line, a package the root pins, and a sibling member, so
`phast add psr/log:^3.0 --no-install` stays fully offline, and so does adding a pinned or
sibling package.

## `phast remove <packages...>`

Remove dependencies and install. Validates that every named package exists before
modifying anything; drops a section that becomes empty.

| Option                | Description                                            |
|-----------------------|--------------------------------------------------------|
| `--dev`               | Remove from `[require-dev]` instead of `[require]`     |
| `--no-install`        | Update `phast.toml` only; skip the install             |
| `--project-dir <dir>` | Project directory (default: current directory)         |
| `--member <path>`     | Target a workspace member's manifest                   |

Like `add`, `remove` is refused at a workspace root, which has no dependencies of its own
to remove: target a member with `--member <path>`.

## `phast lock`

Resolve and write `phast.lock` without downloading anything or generating the
autoloader. A no-op when the lock is already in sync.

| Option                 | Description                                                                    |
|------------------------|--------------------------------------------------------------------------------|
| `--refresh`            | Re-resolve even when the lock is fresh, keeping locked versions where possible |
| `--registry-url <url>` | Resolve against this single registry                                           |
| `--project-dir <dir>`  | Project directory (default: current directory)                                 |

## `phast outdated`

Compare locked versions against the newest available in the registry. Each outdated
package is classified `patch`, `minor`, or `major` (`major` also means: outside your
current constraint). **Exits `1` when anything is outdated**, making it usable as a CI
gate.

| Option                  | Description                                          |
|-------------------------|------------------------------------------------------|
| `--no-dev`              | Check production packages only                       |
| `--format <text\|json>` | Output format (default `text`)                       |
| `--project-dir <dir>`   | Project directory (default: current directory)       |
| `--member <path>`       | Restrict to a workspace member's direct requirements |

There is deliberately no `--registry-url`. `outdated` always asks the repositories the
manifest declares, so the report describes the registries the project actually resolves
against.

## `phast autoload`

Regenerate the autoloader from `phast.lock`, offline: rebuild `vendor/autoload.php`
and the maps under `vendor/phast/`, and relink `vendor/bin/` stubs, using the
installed tree as-is. No resolution, no downloads. Errors when no lock file exists.
Fires the `pre-autoload-dump` and `post-autoload-dump` script hooks.

| Option                           | Description                                                      |
|----------------------------------|------------------------------------------------------------------|
| `--no-dev`                       | Exclude dev autoload rules                                       |
| `--no-scripts`                   | Skip all lifecycle script hooks                                  |
| `-a`, `--classmap-authoritative` | Trust the classmap exclusively; drop the PSR filesystem fallback |
| `--project-dir <dir>`            | Project directory (default: current directory)                   |

## `phast run [script] [-- args...]`

Run a named script from `[scripts]`. Without a script name, lists all available
scripts. Arguments after `--` are appended to the last command of the script.

| Option                | Description                                               |
|-----------------------|-----------------------------------------------------------|
| `--project-dir <dir>` | Project directory (default: current directory)            |
| `--member <path>`     | Run the script from a member's manifest, in its directory |

## `phast show [packages...]`

Show installed packages from `phast.lock` (offline). Without arguments, lists every
package; with arguments, shows a detail view per package including its requirements.

| Option                  | Description                                          |
|-------------------------|------------------------------------------------------|
| `--format <text\|json>` | Output format (default `text`)                       |
| `--no-dev`              | Exclude `[require-dev]` packages                     |
| `--project-dir <dir>`   | Project directory (default: current directory)       |
| `--member <path>`       | Restrict to a workspace member's direct requirements |

`--member` narrows the listing the same way `outdated` does: to that member's direct
requirements, the entries it can act on in its own manifest.

## `phast why <package>`

Show which packages require the given package, reading only `phast.lock` (offline).

| Option                | Description                                        |
|-----------------------|----------------------------------------------------|
| `--tree`              | Render all reverse-dependency paths up to the root |
| `--no-dev`            | Trace production dependencies only                 |
| `--project-dir <dir>` | Project directory (default: current directory)     |
| `--member <path>`     | Scope to a workspace member's manifest             |

In a workspace, a requirement is attributed to the member that declared it rather than to
the root, which requires nothing of its own; each member's manifest is read separately so
the constraint keeps the name of whoever wrote it. `--member` answers for that member
only, since what a sibling requires does not answer what this member requires.

## `phast audit`

Check locked packages against a security advisory database. **Exits `1` when advisories
are found** (after filters), `0` when clean. Always workspace-wide in a workspace.

| Option                        | Description                                               |
|-------------------------------|-----------------------------------------------------------|
| `--provider <packagist\|osv>` | Advisory source (default `packagist`)                     |
| `--no-dev`                    | Audit production packages only                            |
| `--since <YYYY-MM-DD>`        | Only advisories published on or after this date           |
| `--ignore <ID>`               | Suppress an advisory by ID (repeatable, case-insensitive) |
| `--format <text\|json>`       | Output format (default `text`)                            |
| `--project-dir <dir>`         | Project directory (default: current directory)            |

## `phast workspace`

Show the workspace root and all discovered members (name, version, path), marking the
member containing the current directory. The paths it prints are exactly what `--member`
takes elsewhere.

| Option                | Description                                    |
|-----------------------|------------------------------------------------|
| `--project-dir <dir>` | Project directory (default: current directory) |

## `phast fmt`

Format `phast.toml` in canonical style, preserving comments. In a workspace this covers
the root and every member: each member is reported under its `[package].name`, the root
under `root`, which is the only name a manifest declaring no package can be given.
`--member` narrows it to one, and so does running the command from inside a member.

| Option                | Description                                                  |
|-----------------------|--------------------------------------------------------------|
| `--check`             | Verify only: exit `1` with a diff if formatting would change |
| `--project-dir <dir>` | Project directory (default: current directory)               |
| `--member <path>`     | Format one workspace member's manifest                       |

## `phast validate`

Check `phast.toml` offline: parse and type validation (the same pipeline `install`
uses), plus whole-manifest lints such as duplicate dependencies, self-dependencies,
dangling repository references, and orphaned `[sources]` entries. No network, no
resolution, no writes; a fast smoke test for CI, pre-commit hooks, or hand-editing.
**Exits `1` on any error**; warnings are reported but pass unless `--strict`.

In a workspace it checks the root and every member. Checking only the root would report
nothing, since in a workspace all dependencies live in members; the root is checked as
what it declares, its own `[[repositories]]` and `[sources]`. `--member` narrows the run
to one manifest, and so does running the command from inside a member. A standalone
project has no members, so it stays a single check.

Each finding from a member is prefixed with that member's `[package].name`, so one run
covering several manifests says which one it is talking about. Findings from the root, and
from a standalone project, carry no prefix: there is nothing to tell them apart from.

| Option                | Description                                     |
|-----------------------|-------------------------------------------------|
| `--strict`            | Treat warnings as errors (exit non-zero on any) |
| `--project-dir <dir>` | Project directory (default: current directory)  |
| `--member <path>`     | Validate one workspace member's manifest        |

## `phast migrate`

Convert a `composer.json` to `phast.toml`, extracting inline credentials to `auth.toml`.
Non-destructive; see
[Migrating from Composer](../guides/migrate-from-composer.md).

| Option                | Description                                    |
|-----------------------|------------------------------------------------|
| `--force`             | Overwrite an existing `phast.toml`             |
| `--project-dir <dir>` | Project directory (default: current directory) |

## `phast cache <size|clear>`

Inspect or clear the disk cache.

| Option                    | Description                  |
|---------------------------|------------------------------|
| `-m`, `--metadata-only`   | Only the metadata layer      |
| `-a`, `--archives-only`   | Only the archive layer       |

## `phast cli <version|update>`

Manage the Phast executable itself.

### `phast cli version`

Print the running build's identity and provenance: the version and release name,
the build time, the commit it was built from, the `rustc` version, and the target
triple. Useful when reporting an issue. (The top-level `--version` prints just the
version line.)

### `phast cli update`

Update Phast in place to the latest release. It checks the latest published release
and, if it is newer than the running build, downloads the binary for your platform,
verifies its sha256 checksum, and atomically replaces the running executable. If you
are already on the latest version it does nothing.

Linux and macOS only. If the binary lives somewhere you cannot write (for example a
root-owned directory), the update reports a permission error and points you at the
install script to reinstall with the right permissions.
