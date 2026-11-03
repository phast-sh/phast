//! One test per workflow, named after the feature it covers.
//!
//! Each workflow is a happy path: the sequence a user actually runs, asserted end to
//! end against the shipped binary. They exist so that a refactor which breaks a
//! workflow fails here, by name, rather than in someone's project.
//!
//! Adding one is a directory under `workflows/` plus its name in the list below. See
//! `workflows/README.md` for the spec format.

use std::path::PathBuf;

use phast_test_support::workflow::Workflow;

/// Declare one test per workflow, named after its directory.
///
/// The name is written once, as an identifier, and `stringify!` turns it into the
/// directory to load. Wrappers written by hand repeat it twice, which puts
/// `fn remove() { run("add") }` one copy-paste away: `add` would run twice, `remove`
/// would never run, and both tests would report green.
macro_rules! workflows {
    ($($name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                run(stringify!($name));
            }
        )*
    };
}

// Every workflow should be registered here. One entry per directory under `workflows/`.
// The identifier is both the test name and the directory to load,
// so the two can never drift apart; a directory left out of this list simply never runs.
workflows![
    init, add, remove, install, update, autoload, workspace, migrate
];

/// Load and run `workflows/{name}`, failing the test with the workflow's diagnostics.
fn run(name: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("workflows")
        .join(name);

    let workflow = Workflow::load(&dir).unwrap_or_else(|error| panic!("{error:?}"));

    // A workflow can need something this machine does not have: a live registry, which is
    // opt-in so the offline ones stay usable on a pull request, or a PHP interpreter.
    // libtest cannot mark a test ignored at runtime, so say why and pass.
    if let Some(reason) = workflow.skip_reason() {
        eprintln!("skipping '{name}': {reason}");
        return;
    }

    workflow.run().unwrap_or_else(|error| panic!("{error:?}"));
}
