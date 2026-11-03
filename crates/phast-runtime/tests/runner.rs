use phast_core::manifest::Script;
use phast_runtime::runner::ScriptEntry;

/// The command `steps` run as a sequence, the list form of a `[scripts]` entry.
fn steps(steps: &[&str]) -> Script {
    Script::Multiple(steps.iter().map(|step| (*step).to_owned()).collect())
}

// ---------------------------------------------------------------------------
// Classifying a script string
// ---------------------------------------------------------------------------

#[test]
fn an_at_php_prefix_makes_a_php_command() {
    let entry: ScriptEntry = "@php artisan migrate".parse().unwrap();
    assert_eq!(entry, ScriptEntry::PhpCommand("artisan migrate".to_owned()));
}

#[test]
fn a_double_colon_makes_a_php_callback() {
    let entry: ScriptEntry = "Illuminate\\Foundation\\ComposerScripts::postAutoloadDump"
        .parse()
        .unwrap();
    assert_eq!(
        entry,
        ScriptEntry::PhpCallback(
            "Illuminate\\Foundation\\ComposerScripts::postAutoloadDump".to_owned(),
        ),
    );
}

#[test]
fn anything_else_is_a_shell_command() {
    let entry: ScriptEntry = "echo hello".parse().unwrap();
    assert_eq!(entry, ScriptEntry::Shell("echo hello".to_owned()));
}

#[test]
fn the_at_php_prefix_is_checked_before_the_double_colon() {
    // Order decides this one: a `@php` invocation naming a static method is a
    // command to run, not a callback to reflect into.
    let entry: ScriptEntry = "@php Foo::bar".parse().unwrap();
    assert_eq!(entry, ScriptEntry::PhpCommand("Foo::bar".to_owned()));
}

#[test]
fn a_bare_at_php_is_left_to_the_shell() {
    // The prefix includes its trailing space, so `@php` with nothing after it is
    // not a PHP command; it goes to the shell and fails there, where the user
    // sees the command they actually wrote.
    let entry: ScriptEntry = "@php".parse().unwrap();
    assert_eq!(entry, ScriptEntry::Shell("@php".to_owned()));
}

// ---------------------------------------------------------------------------
// Expanding a manifest entry into steps
// ---------------------------------------------------------------------------

#[test]
fn a_single_command_expands_to_one_entry() {
    let entries = ScriptEntry::from_def(&Script::Single("echo single".to_owned()));
    assert_eq!(entries, vec![ScriptEntry::Shell("echo single".to_owned())]);
}

#[test]
fn a_list_expands_in_declaration_order() {
    let entries = ScriptEntry::from_def(&steps(&["echo a", "@php artisan foo"]));
    assert_eq!(
        entries,
        vec![
            ScriptEntry::Shell("echo a".to_owned()),
            ScriptEntry::PhpCommand("artisan foo".to_owned()),
        ],
    );
}

#[test]
fn a_list_classifies_each_command_on_its_own() {
    // One entry may mix all three kinds, so classification is per command rather
    // than decided once for the whole sequence.
    let entries = ScriptEntry::from_def(&steps(&["echo a", "@php artisan b", "Acme\\Hooks::c"]));
    assert_eq!(
        entries,
        vec![
            ScriptEntry::Shell("echo a".to_owned()),
            ScriptEntry::PhpCommand("artisan b".to_owned()),
            ScriptEntry::PhpCallback("Acme\\Hooks::c".to_owned()),
        ],
    );
}

#[test]
fn an_empty_list_expands_to_no_entries() {
    // A caller runs the result as a sequence either way, so an entry declaring
    // nothing is a no-op rather than something to guard against.
    assert!(ScriptEntry::from_def(&steps(&[])).is_empty());
}
