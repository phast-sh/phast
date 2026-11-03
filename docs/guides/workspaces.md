---
description: "Workspaces are Phast's native monorepo support: several PHP packages in one repository, resolved together with one phast.lock and one vendor/."
---

# Workspaces

Workspaces are Phast's native monorepo support: several PHP packages developed together
in one repository, resolved together, sharing a single `phast.lock` and a single
`vendor/` directory. There is nothing to wire up per member, no path repositories and no
merge plugins.

## Layout

```
my-project/
├── phast.toml           # workspace root (coordinator only)
├── phast.lock           # one lock for the whole workspace
├── vendor/              # one vendor dir, shared by all members
├── packages/
│   ├── http-client/
│   │   └── phast.toml
│   └── api-sdk/
│       └── phast.toml
└── tools/
    └── codegen/
        └── phast.toml
```

The root `phast.toml` declares the workspace:

```toml
[workspace]
members = ["packages/*", "tools/codegen"]
exclude = ["packages/legacy"]

[workspace.dependencies]
"symfony/http-foundation" = "^7.0"
"phpunit/phpunit"         = "^11.0"
```

`members` and `exclude` are glob patterns relative to the root. A directory that matches
a member pattern counts as a member only if it contains a `phast.toml`.

!!! note "The root is a pure coordinator"
    A workspace root must not declare `[package]`, `[require]`, or `[require-dev]`;
    Phast refuses to load it if it does. Shared versions belong in
    `[workspace.dependencies]`, and each member opts in from its own manifest.

## Member manifests

A member is an ordinary Phast package; its manifest is valid standalone, which keeps
members independently publishable and deployable:

```toml
# packages/api-sdk/phast.toml
[package]
name    = "my-org/api-sdk"
version = "1.0.0"

[require]
php                       = "^8.2"
"my-org/http-client"      = "^1.0"               # sibling member
"symfony/http-foundation" = { workspace = true } # inherited constraint

[require-dev]
"phpunit/phpunit" = { workspace = true }
```

Two things to notice:

- **Sibling dependencies** are declared by name like any other package. Phast satisfies
  them from the local source tree instead of a registry; member code is never copied
  into `vendor/`, the generated autoloader points straight at each member's sources, so
  changes are visible immediately with no reinstall.
- **`{ workspace = true }`** inherits the constraint from `[workspace.dependencies]`.
  Bumping a shared dependency is then a one-line change at the root instead of an edit
  in every member. Which section the reference appears in still decides whether it is a
  runtime or dev dependency for that member.

## One resolution, one lock

Running `phast install` (from the root or anywhere inside the workspace; Phast walks up
to find the root) merges the requirements of all members and resolves them together.
Every member gets the same version of every shared package, and the result is pinned in
the single root `phast.lock`.

If two members declare *different* constraints for the same external package, the merge
fails up front and names both sides:

```
workspace dependency conflict for "symfony/console"
  packages/api requires "^7.0"
  packages/legacy requires "^6.0"

  hint: pin a shared constraint in [workspace.dependencies] to resolve this
```

This is deliberate: a silently chosen winner in a shared lock causes surprises later;
the error makes the team decide once, at the root.

Members may depend on one another, but not in a cycle. If member A requires B and B
requires A (directly or through a chain), Phast rejects the workspace when it loads,
printing the cycle, instead of looping during resolution:

```
workspace members form a require cycle: my-org/api-sdk -> my-org/http-client -> my-org/api-sdk

  hint: break the cycle, or move one edge to [require-dev]
```

The check covers production `require` edges only, so moving one direction of the
dependency to `[require-dev]` is a valid way to break the cycle.

## Inspecting the workspace

```console
$ phast workspace

Workspace root: /work/my-project

Members (3)
  my-org/http-client  1.0.0  packages/http-client
* my-org/api-sdk      1.0.0  packages/api-sdk
  my-org/codegen      0.1.0  tools/codegen
```

The `*` marks the member containing your current directory.

## Working on a single member

Commands that edit a manifest or run scripts take `--member <path>` (the path relative
to the root, exactly as `phast workspace` prints it):

```console
$ phast add guzzlehttp/guzzle:^7.0 --member packages/http-client
$ phast remove psr/log --member packages/api-sdk
$ phast run test --member packages/api-sdk
$ phast install --member packages/api-sdk
```

Dependencies always belong to a member. The root is a coordinator with no `[require]` of
its own, so `phast add` and `phast remove` refuse to run against it:

```
a workspace root has no dependencies of its own. Target a member with --member <path>
```

Scaffolding a new member is just `phast init` with a path; it is picked up automatically
if it matches a `members` pattern:

```console
$ phast init packages/new-service
```

### What `add` records inside a workspace

Adding to a member is not always a question for a registry. Before resolving anything,
Phast checks what the workspace already answers for the package:

**The root pins it in `[workspace.dependencies]`.** The member records
`{ workspace = true }`, not a copy of the constraint:

```console
$ phast add symfony/http-foundation --member packages/api-sdk
```

```toml
[require]
"symfony/http-foundation" = { workspace = true }
```

Giving a constraint that disagrees with the pin is refused rather than quietly narrowed,
because only the root decides what every member gets:

```
the workspace pins 'symfony/http-foundation' at "^7.0". Drop the constraint to inherit
it, or change the pin at the root
```

**A sibling member declares it.** The constraint is derived as `^major.minor` from that
member's own `[package].version`, with no registry involved: a sibling is satisfied from
the source tree, so there is nothing to look up and nothing to solve for. With
`my-org/http-client` at version `1.2.3`, `phast add my-org/http-client --member
packages/api-sdk` records `^1.2`. A constraint you give by hand wins, being the more
specific answer. A sibling that declares no `[package].version` is an error rather than a
guess:

```
workspace member 'my-org/http-client' declares no [package].version to derive a
constraint from. Add one, or pass a constraint: my-org/http-client:^1.0
```

Because both cases are answered without a registry, they stay offline the same way an
explicit constraint does: `phast add my-org/http-client --no-install` touches the network
no more than `phast add psr/log:^3.0 --no-install` does.

Read-only commands scope themselves sensibly:

- `phast why`, `phast outdated`, and `phast show` accept `--member <path>`, and when run
  from *inside* a member directory they scope to that member automatically. For `outdated`
  and `show`, that narrows the report to the member's direct requirements, the ones it can
  actually change in its own manifest. `phast why` narrows a different thing: whose
  requirements top the chain. Its requirers still come from the whole shared lock, since
  what an installed package requires does not change with the member you asked from. At
  the root, `phast why` attributes each requirement to the member that declared it rather
  than to the root, which requires nothing of its own.
- `phast audit` is always workspace-wide, no matter where it runs: everything in the
  shared lock is physically installed in the shared `vendor/`, so a per-member "no
  advisories" answer would be false assurance.
- `phast run` takes `--member <path>` and, like the others, scopes to the member
  containing your current directory when you are inside one.

`phast update` takes `--member <path>` too, and uses it for a different question: which
packages may move. Given no package names, a member supplies the list from its own
requirements, so `phast update --member packages/api-sdk` updates what that member
requires and leaves the rest of the workspace pinned. Naming packages explicitly wins.
The lock stays workspace-wide either way.

## Checking every manifest

`phast validate` and `phast fmt` cover the whole workspace: the root and every member.
Checking only the root would report almost nothing, since in a workspace all the
dependencies live in members. Members are named by their `[package].name` in the output;
the root is not, having no package name to be named by. `--member` narrows either command
to one manifest, and so does running it from inside a member's directory:

```console
$ phast validate                             # root + every member
$ phast validate --member packages/api-sdk   # just this one
$ phast fmt --check                          # CI mode, whole workspace
```

## Scripts and lifecycle hooks

Both levels can declare `[scripts]`, and they mean different things.

**Lifecycle hooks belong to the root.** `post-autoload-dump` and its siblings fire once per
command, so only the root's are run, and a hook declared by a member is ignored as a hook. An
event that happened once should not run a task once per member, in an order nobody chose.

**Named tasks belong wherever you run them.** A task in a member's `[scripts]` runs with
`--member`, or from inside that member's directory. A task at the root runs from the root.

```toml title="phast.toml (workspace root)"
[scripts]
post-autoload-dump = "php artisan package:discover"   # fires on install and update
lint               = "phast run lint --member packages/api"
```

```toml title="packages/api/phast.toml"
[scripts]
lint = "vendor/bin/phpstan analyse"   # phast run lint --member packages/api
```

A member that needs work done after every install has one option: call it from the root's
hook, as `lint` does above. That keeps the order explicit and visible in one file.

!!! note
    `phast install --member <path>` scopes the *download* to one member's dependency
    closure, which is useful for building a single service in CI. The lock still covers
    the whole workspace and is unchanged; only which archives are fetched into the shared
    `vendor/` is narrowed.
