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

## Command forms

| Form            | Example                | How it runs                                         |
|-----------------|------------------------|-----------------------------------------------------|
| `@php ...`      | `@php artisan migrate` | The resolved PHP binary, with the rest as arguments |
| `@name`         | `@lint`                | The script called `lint`, in its place              |
| Static callback | `Vendor\Class::method` | Phast loads the class and calls the method          |
| Shell command   | `cp .env.example .env` | Passed to `sh -c`, so shell syntax works            |

`@php` is resolved to the `PHAST_PHP_BIN` environment variable when set, otherwise
`php` on your `PATH`; the same binary Phast uses for platform detection. Static
callbacks are covered in [PHP callbacks](#php-callbacks) below.

A script can run another one with `@name`, and that one can run others in turn:

```toml
[scripts]
lint = "vendor/bin/phpstan analyse"
test = ["@lint", "@php vendor/bin/phpunit"]
```

`phast run test` runs `lint` first, then PHPUnit.

!!! note
    Scripts that invoke the `composer` binary only work if Composer happens to be
    installed.

## PHP callbacks

A callback is a public static method, written the way Composer packages write them:

```toml
[scripts]
pre-autoload-dump = "Aws\\Script\\Composer\\Composer::removeUnusedServices"

[extra]
"aws/aws-sdk-php" = ["S3"]
```

```php
use Composer\Script\Event;

public static function removeUnusedServices(Event $event): void
{
    $extra = $event->getComposer()->getPackage()->getExtra();
    $names = $extra['aws/aws-sdk-php'] ?? [];
    $event->getIO()->write('Keeping AWS services: ' . implode(', ', $names));
}
```

Phast finds the class through the autoload rules of the installed packages, so a
callback runs even on a first install, before `vendor/autoload.php` exists. It doesn't
load classes listed only in a `classmap`, or `files` entries.

The event gives a callback:

| Method                                          | Returns                                                                                                           |
|-------------------------------------------------|-------------------------------------------------------------------------------------------------------------------|
| `getName()`                                     | The event, or the script's name under `phast run`                                                                 |
| `getArguments()`                                | The arguments after `--` in `phast run`                                                                           |
| `isDevMode()`                                   | `false` under `--no-dev`                                                                                          |
| `getIO()->write()`, `writeError()`              | Prints to the terminal                                                                                            |
| `getIO()->ask()`, `askConfirmation()`           | The default answer; Phast never stops to ask                                                                      |
| `getComposer()->getPackage()`                   | The root package: `getName()`, `getVersion()`, `getExtra()`, `getRequires()`, `getDevRequires()`, `getAutoload()` |
| `getComposer()->getConfig()->get('vendor-dir')` | The absolute vendor path; `bin-dir` works too                                                                     |

A callback that calls anything else stops with a message naming the method:

```console
Script Acme\Hooks\Report::locker (post-install-cmd) needs Composer\Composer::getLocker(), which Phast does not provide to scripts yet.
Skip scripts with --no-scripts, and please report it: https://github.com/phast-sh/phast/issues
```

And if the class can't be found, Phast says which one:

```console
Script Acme\Hooks\Missing::run (post-install-cmd) can't run: class Acme\Hooks\Missing not found.
```

## Scripts written as a map

Symfony projects have a script written as a map rather than a command or a list: each
command is mapped to how Symfony Flex, a Composer plugin, should run it.

```toml
[scripts]
post-install-cmd = ["@auto-scripts"]

[scripts.auto-scripts]
"cache:clear" = "symfony-cmd"
"assets:install %PUBLIC_DIR%" = "symfony-cmd"
```

Phast reads and keeps these, but doesn't run them. `phast install` and `phast update` say
so, and list the commands so you can run them yourself:

```console
Skipping 'auto-scripts': Phast doesn't support map scripts. Run these yourself if you need them: 'cache:clear', 'assets:install %PUBLIC_DIR%'
```

For a Symfony project, those are console commands: `php bin/console cache:clear`.

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
