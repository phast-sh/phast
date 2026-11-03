# Workflows

One directory per workflow, holding a real project and a `workflow.toml` stating what Phast
should do to it. Each is a happy path for one feature: the sequence a user actually
runs, asserted end to end against the shipped binary.

## What belongs here

This tier is deliberately small. Unit tests cover library APIs and `testing/cli` covers
every command's contract against a mocked registry, so a workflow that restates either is
cost without coverage. A workflow earns its place only when both of these hold:

1. **Breaking it blocks a user's core loop.** Not "this command is useful", but "a user
   with this broken cannot install, cannot adopt Phast, or silently loses work".
2. **The failure is invisible to `testing/cli`.** In practice that means one of: a sequence
   where one command consumes what another wrote, a file asserted byte for byte, or a real
   `vendor/` tree.

The second clause is the one that gets forgotten. A command can be genuinely critical and
still belong entirely to `testing/cli`: `validate` is load-bearing for every other
workflow here, and every assertion worth making about it is an exit code or a message,
which is exactly what the cli tier exists for. Criticality decides whether to cover
something at all; clause 2 decides whether it is covered *here*.

What that leaves:

| Workflow    | Blocks                        | Needs this tier because                      |
|-------------|-------------------------------|----------------------------------------------|
| `init`      | starting a project            | the emitted manifest is asserted as bytes    |
| `add`       | changing dependencies         | comments and alignment survive an edit       |
| `remove`    | changing dependencies         | an emptied section is dropped, in place      |
| `install`   | everything                    | lock, autoload, vendor tree, generated PHP   |
| `update`    | moving versions               | resolution against a live registry           |
| `autoload`  | every class a project loads   | a real interpreter loads what Phast wrote    |
| `workspace` | multi-package repos           | members installed for real, and no fallback  |
| `migrate`   | adopting Phast at all         | the emitted manifest is then fed to Phast    |

`lock` has no workflow of its own on purpose: it is the first step of `install`, which is
the honest shape of its contract, since what it is for is feeding the next command.

`autoload` has one because its flags are not observable as text. `--no-dev` drops a rule
from a map, and whether that matters is a question about what an interpreter can then
resolve, which is why that workflow asserts by running PHP rather than by reading the maps
it wrote. It is the only workflow that executes PHP at all.

Commands left out are not untested, they are tested one tier down. `fmt` is the worked
example: a broken formatter is cosmetic and loud, and `testing/cli/tests/fmt.rs` already
pins sorting, idempotence, and that `--check` writes nothing.

Adding one is a directory here, plus its name in the list in `tests/workflows.rs`:

```rust
workflows![add, remove];
```

That expands to one `#[test]` per name, so `cargo test -p phast-e2e-tests add` runs
just that workflow and a failure reads `test add ... FAILED`.

The identifier is also the directory to load, so the two can never drift apart. It does
mean directory names have to be valid Rust identifiers: use `platform_check`, not
`platform-check`. A directory left out of the list simply never runs.

## Layout

Two names inside a workflow directory are reserved:

| Name            | Meaning                                                       |
|-----------------|---------------------------------------------------------------|
| `workflow.toml` | The spec. Required.                                           |
| `snapshots/`    | Expected output, compared by a step with `matches = true`.    |

Everything else is input, copied verbatim into a scratch directory before the first
step runs. Workflow directories are never written to.

Commands that generate their own manifest keep it under `snapshots/`, not beside the
spec, where it would read as an input.

```
workflows/conflict/        workflows/add/
├── workflow.toml          ├── workflow.toml
├── phast.toml   (input)   ├── phast.toml              (input)
└── src/         (input)   └── snapshots/
                               └── phast.toml.snap     (output)
```

Files under `snapshots/` end in `.snap`, and the suffix is stripped when resolving
what they compare against: `phast.toml.snap` is checked against `phast.toml`. They
are compared byte for byte, which makes them precisely the files an editor or a
formatter must not touch, and an extension no formatter recognizes is the only
reliable way to keep them out of reach. Generate a snapshot by running the real steps
and copying the result, never by hand.

## Spec

```toml
description = "A [conflict] entry forces resolution away from the newest major."
tier        = "focused"   # focused (default) | ecosystem
network     = true        # requires PHAST_TEST_NETWORK to run
php         = true        # requires a php interpreter, and is skipped without one

[[step]]
args   = ["install", "--dry-run"]
stdout = ["psr/log"]

[[step]]
args    = ["install"]
creates = ["phast.lock", "vendor/autoload.php"]
```

Steps run in order against one scratch directory, so each sees what the previous left
behind. A step asserts exit code 0 unless it declares otherwise.

| Key       | Asserts                                                       |
|-----------|---------------------------------------------------------------|
| `args`    | Arguments to the binary, without the program name. Required.  |
| `status`  | Exit code. Defaults to `0`.                                   |
| `stdout`  | Substrings that must appear on stdout.                        |
| `stderr`  | Substrings that must appear on stderr.                        |
| `creates` | Paths that must exist after the step.                         |
| `absent`  | Paths that must not exist after the step.                     |
| `matches` | Every snapshot matches its workdir counterpart.               |

Unknown keys are rejected, so a typo fails the workflow rather than silently asserting
nothing.

## Running

```sh
make test-e2e                                  # offline workflows only
make test-e2e NETWORK=1                        # including those needing a registry
make test-e2e TEST=install                     # one workflow by name
make test-e2e TEST="update -- --nocapture"     # and report what it skipped, and why
PHAST_BIN=target/release/phast make test-e2e   # against a prebuilt binary
```

Workflows with `network = true` resolve against a live registry, so their assertions
are constraint-shaped (`<3.0`) rather than exact versions: one pinned to `2.0.0` breaks
the day `2.0.1` is published. A workflow needing an exact version belongs in
`testing/cli` with a mocked registry instead.

A skipped workflow still reports `ok`, because a test cannot be marked ignored once it is
running, so `--nocapture` is the only way to tell a skip from a pass. That matters most for
`php = true`: a registry is opted in to deliberately, while an interpreter is detected, so
that coverage can quietly not run on a machine that never asked for it.
