---
description: "The [scripts] section of phast.toml: lifecycle hooks Phast runs around install and update, and named tasks you run with phast run."
---

# Running scripts

The `[scripts]` section of `phast.toml` holds two kinds of entries: **lifecycle hooks**
that Phast fires automatically around install and update, and **named tasks** you run
explicitly with `phast run`.

```toml
[scripts]
post-install-cmd = "@php artisan optimize"
post-autoload-dump = [
    "Illuminate\\Foundation\\ComposerScripts::postAutoloadDump",
    "@php artisan package:discover --ansi",
]
test = "@php vendor/bin/phpunit"
lint = "vendor/bin/phpstan analyse"
```

A value is either a single command string or an array of commands run in sequence. The
sequence stops at the first command that exits non-zero, and the Phast command that
triggered it fails with that exit status.

## The three command forms

| Form            | Example                | How it runs                                                    |
|-----------------|------------------------|----------------------------------------------------------------|
| `@php ...`      | `@php artisan migrate` | The resolved PHP binary, with the rest as arguments            |
| Static callback | `Vendor\Class::method` | A short PHP bootstrap loads `vendor/autoload.php` and calls it |
| Shell command   | `cp .env.example .env` | Passed to `sh -c`, so shell syntax works                       |

`@php` is resolved to the `PHAST_PHP_BIN` environment variable when set, otherwise
`php` on your `PATH`; the same binary Phast uses for platform detection. Static
callbacks receive a minimal `Composer\Script\Event` stub when the real class is not
installed, so common Composer-era callbacks run unchanged.

!!! note
    Script references (a script whose value is `"@another-script"`) are not supported
    yet; inline the commands instead. Scripts that invoke the `composer` binary only
    work if Composer happens to be installed.

## Lifecycle hooks

Phast fires these events automatically; define any of them in `[scripts]` to hook in:

| Event                | Fired                                                    |
|----------------------|----------------------------------------------------------|
| `pre-install-cmd`    | before `phast install` starts                            |
| `post-install-cmd`   | after `phast install` completes                          |
| `pre-update-cmd`     | before `phast update` starts                             |
| `post-update-cmd`    | after `phast update` completes                           |
| `pre-autoload-dump`  | before the autoloader is regenerated                     |
| `post-autoload-dump` | after the autoloader is regenerated (install and update) |

Events you do not define are skipped silently, and so are event names Phast does not
recognize, so a manifest migrated from Composer with extra hooks keeps working.

Pass `--no-scripts` to `phast install` or `phast update` to skip every hook, useful in
CI stages where the hooks have already run or are not wanted.

`--dry-run` runs no hooks either. It only reports what would change, and a hook is code
the project ships, so a dry run of a repository you have just cloned runs nothing of it.

In a workspace, hooks are declared at the root and fire once. A hook in a member's
`[scripts]` is ignored as a hook, since an event that happened once should not run a task
once per member. See [Workspaces](workspaces.md#scripts-and-lifecycle-hooks).

## Named tasks

Anything in `[scripts]` that is not a lifecycle event is a task you run by name:

```console
$ phast run test
$ phast run lint
```

Arguments after `--` are appended to the *last* command of the script:

```console
$ phast run test -- --filter=UserTest
```

Run `phast run` with no arguments to list every available script:

```console
$ phast run

  lint  vendor/bin/phpstan analyse
  test  @php vendor/bin/phpunit
```

Before each command executes, Phast prints it prefixed with `>`, so the output always
shows what actually ran.

In a workspace, `phast run <script> --member packages/api` runs the script from that
member's manifest, inside the member's directory. See [Workspaces](workspaces.md).
