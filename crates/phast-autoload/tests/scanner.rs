use phast_autoload::scanner::{Scanner, extract};

use indoc::indoc;

/// The smallest scannable PHP file: one class, `App\Mock`, and nothing else.
const CLASS_PHP: &str = indoc! {r#"
    <?php

    namespace App;

    class Mock { }
"#};

// ---------------------------------------------------------------------------
// Scanning a path
// ---------------------------------------------------------------------------

#[test]
fn scan_path_errors_on_missing_path() {
    let mut scanner = Scanner::new(None);
    let result = scanner.scan_path(std::path::Path::new(
        "/nonexistent/path/that/does/not/exist",
    ));
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("does not exist"),
        "error should mention the path: {msg}",
    );
}

#[test]
fn scan_path_accepts_a_single_file() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("Solo.php");
    std::fs::write(&file, CLASS_PHP).unwrap();

    let mut scanner = Scanner::new(None);
    scanner.scan_path(&file).expect("scan");

    // A `classmap` entry may name a file rather than a directory, so both shapes
    // reach the same scanner.
    let found: Vec<String> = scanner.into_iter().map(|(class, _)| class).collect();
    assert_eq!(found, vec!["App\\Mock"]);
}

#[test]
fn only_php_and_inc_files_are_scanned() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("legacy.inc"), CLASS_PHP).unwrap();
    std::fs::write(dir.path().join("notes.txt"), CLASS_PHP).unwrap();
    std::fs::write(dir.path().join("view.phtml"), CLASS_PHP).unwrap();

    let mut scanner = Scanner::new(None);
    scanner.scan_path(dir.path()).expect("scan");

    // `.inc` counts, and `.phtml` does not despite holding PHP: a template is not
    // where a class lives, and scanning one would put view code in the classmap.
    let found: Vec<String> = scanner.into_iter().map(|(class, _)| class).collect();
    assert_eq!(found, vec!["App\\Mock"], "one file, scanned once");
}

#[test]
fn a_file_with_non_utf8_bytes_is_still_scanned() {
    let dir = tempfile::tempdir().expect("temp dir");
    // A latin-1 byte in a comment, which PHP files in the wild do carry. Reading
    // this strictly as UTF-8 would fail the whole scan over a byte nothing reads.
    let source = b"<?php\nnamespace App;\n// caf\xe9\nclass Real { }\n";
    std::fs::write(dir.path().join("Real.php"), source).unwrap();

    let mut scanner = Scanner::new(None);
    scanner.scan_path(dir.path()).expect("scan");

    let found: Vec<String> = scanner.into_iter().map(|(class, _)| class).collect();
    assert_eq!(found, vec!["App\\Real"]);
}

// ---------------------------------------------------------------------------
// Declarations that are found
// ---------------------------------------------------------------------------

#[test]
fn a_class_is_qualified_with_its_namespace() {
    let src = indoc! {r#"
        <?php

        namespace Foo\Bar;

        class Baz { }
    "#};
    assert_eq!(extract(src), vec!["Foo\\Bar\\Baz"]);
}

#[test]
fn an_interface_and_a_trait_are_found() {
    let src = indoc! {r#"
        <?php

        namespace Acme;

        interface Contract { }
        trait Helper { }
    "#};
    assert_eq!(extract(src), vec!["Acme\\Contract", "Acme\\Helper"]);
}

#[test]
fn a_backed_enum_is_found() {
    let src = indoc! {r#"
        <?php

        namespace App;

        enum Suit: string {
            case Hearts = 'H';
        }
    "#};
    assert_eq!(extract(src), vec!["App\\Suit"]);
}

#[test]
fn a_class_outside_any_namespace_is_unqualified() {
    let src = indoc! {r#"
        <?php

        class GlobalClass { }
    "#};
    assert_eq!(extract(src), vec!["GlobalClass"]);
}

#[test]
fn several_declarations_keep_their_order() {
    let src = indoc! {r#"
        <?php

        namespace App;

        class A { }
        interface B { }
        trait C { }
        enum D { }
    "#};
    assert_eq!(extract(src), vec!["App\\A", "App\\B", "App\\C", "App\\D"]);
}

#[test]
fn a_semi_reserved_word_is_a_valid_class_name() {
    // `Enum` (and `Match`, `Fn`, `Readonly`) are legal PHP class names,
    // e.g. Laravel's Illuminate\Validation\Rules\Enum.
    let src = indoc! {r#"
        <?php

        namespace Illuminate\Validation\Rules;

        class Enum { }
    "#};
    assert_eq!(extract(src), vec!["Illuminate\\Validation\\Rules\\Enum"]);
}

// ---------------------------------------------------------------------------
// Look-alikes that are not declarations
// ---------------------------------------------------------------------------

#[test]
fn an_anonymous_class_is_ignored() {
    let src = indoc! {r#"
        <?php

        $x = new class { };
    "#};
    assert!(extract(src).is_empty());
}

#[test]
fn variable_named_class_is_not_a_declaration() {
    // `$class` must never be read as the `class` keyword. This was the dominant
    // false-positive source: `Class_ $class): array` yielded a bogus class `array`,
    // and `$class->methods` yielded `methods`.
    let src = indoc! {r#"
        <?php

        function parentClasses(array $details, Class_ $class): array {
            foreach ($class->methods as $name => $method) {
                //
            }

            return $class->namespace;
        }
    "#};
    assert!(extract(src).is_empty(), "got: {:?}", extract(src));
}

#[test]
fn class_magic_constant_is_not_a_declaration() {
    // `Foo::class` in a switch arm produced bogus classes from the arm body
    // (`case X::class: assert(...)` yielded `assert`).
    let src = indoc! {r#"
        <?php

        namespace App;

        class Real { }

        switch ($x) {
            case Other::class:
                assert($ok);
                break;
        }
    "#};
    assert_eq!(extract(src), vec!["App\\Real"]);
}

#[test]
fn object_property_named_class_is_not_a_declaration() {
    let src = indoc! {r#"
        <?php

        $obj->class;
        $obj?->class;

        class Real { }
    "#};
    assert_eq!(extract(src), vec!["Real"]);
}

#[test]
fn attribute_is_not_a_line_comment() {
    // `#[...]` is an attribute, not a `#` comment; the class on the next line must
    // still be found, and a `::class` inside the attribute must not leak.
    let src = indoc! {r#"
        <?php

        #[CoversClass(Target::class)]
        class Real { }
    "#};
    assert_eq!(extract(src), vec!["Real"]);
}

// ---------------------------------------------------------------------------
// Strings and heredocs
// ---------------------------------------------------------------------------

#[test]
fn a_keyword_in_a_string_is_ignored() {
    let src = indoc! {r#"
        <?php

        namespace App;

        class RealClass { }

        $s = "class NotAClass";
    "#};
    assert_eq!(extract(src), vec!["App\\RealClass"]);
}

#[test]
fn a_heredoc_body_is_ignored() {
    let src = indoc! {r#"
        <?php

        $tpl = <<<EOT
        class Fake { }
        EOT;

        class Real { }
    "#};
    assert_eq!(extract(src), vec!["Real"]);
}

#[test]
fn a_nowdoc_body_is_ignored() {
    let src = indoc! {r#"
        <?php

        $tpl = <<<'EOT'
        class Fake { }
        EOT;

        class Real { }
    "#};
    assert_eq!(extract(src), vec!["Real"]);
}

#[test]
fn an_indented_heredoc_close_ends_the_body() {
    // PHP 7.3+ allows the closing label to be indented. The indentation below is the
    // fixture, not layout: dedenting it away would remove what this test is about.
    let src = indoc! {r#"
        <?php

        $tpl = <<<EOT
            class Fake { }
            EOT;

        class Real { }
    "#};
    assert_eq!(extract(src), vec!["Real"]);
}

// ---------------------------------------------------------------------------
// Inline HTML
// ---------------------------------------------------------------------------

#[test]
fn leading_inline_html_is_ignored() {
    // A template file: HTML before the first `<?php`. Tokens like `div` and the `class`
    // attribute must not be read as declarations.
    let src = indoc! {r#"
        <div class="align"><h1>Title</h1></div>
        <?php

        namespace App;

        class Real { }
    "#};
    assert_eq!(extract(src), vec!["App\\Real"]);
}

#[test]
fn html_after_close_tag_is_ignored() {
    let src = indoc! {r#"
        <?php class Real {} ?>
        <button class="btn">click</button>
    "#};
    assert_eq!(extract(src), vec!["Real"]);
}

#[test]
fn php_is_reentered_after_an_html_block() {
    let src = indoc! {r#"
        <?php class A {} ?>
        <div>text</div>
        <?php class B {}
    "#};
    assert_eq!(extract(src), vec!["A", "B"]);
}

#[test]
fn pure_html_file_declares_nothing() {
    let src = indoc! {r#"
        <div class="a"><span data-x="1">h1</span></div>
    "#};
    assert!(extract(src).is_empty(), "got: {:?}", extract(src));
}

// ---------------------------------------------------------------------------
// Duplicates
// ---------------------------------------------------------------------------

#[test]
fn conditional_class_definition_is_returned_once() {
    // A class defined once per branch of an if/else (the standard polyfill
    // pattern) is a single type statically, so it appears once, not per branch.
    let src = indoc! {r#"
        <?php

        namespace Pdo;

        if (\PHP_VERSION_ID < 80400) {
            class Mysql extends \PDO {}
        } else {
            class Mysql extends \PDO {}
        }
    "#};
    assert_eq!(extract(src), vec!["Pdo\\Mysql"]);
}

#[test]
fn conditional_definition_in_one_file_is_not_a_duplicate() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = indoc! {r#"
        <?php

        namespace Pdo;

        if (true) {
            class Mysql {}
        } else {
            class Mysql {}
        }
    "#};

    std::fs::write(dir.path().join("Mysql.php"), src).unwrap();

    let mut scanner = Scanner::new(None);
    scanner.scan_path(dir.path()).expect("scan");

    assert!(
        scanner.duplicates().is_empty(),
        "same class in the same file is not a duplicate: {:?}",
        scanner.duplicates(),
    );
}

#[test]
fn same_class_in_two_files_is_recorded_as_a_duplicate() {
    let dir = tempfile::tempdir().expect("temp dir");

    std::fs::write(dir.path().join("a.php"), CLASS_PHP).unwrap();
    std::fs::write(dir.path().join("b.php"), CLASS_PHP).unwrap();

    let mut scanner = Scanner::new(None);
    scanner.scan_path(dir.path()).expect("scan");

    let duplicates = scanner.duplicates();
    assert_eq!(duplicates.len(), 1, "expected one duplicate");
    assert_eq!(duplicates[0].class, "App\\Mock");

    // Which of the two is kept is whatever `read_dir` yielded first, so only the
    // pairing is asserted here, not which file won.
    assert_ne!(duplicates[0].kept, duplicates[0].ignored);
}

#[test]
fn a_file_reached_twice_is_scanned_once() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("One.php");
    std::fs::write(&file, CLASS_PHP).unwrap();

    // A package may list a directory and a file inside it in the same `classmap`.
    // Without the visited guard the second pass finds the class again and reports
    // the file as a duplicate of itself.
    let mut scanner = Scanner::new(None);
    scanner.scan_path(dir.path()).expect("scan dir");
    scanner.scan_path(&file).expect("scan file");

    assert!(
        scanner.duplicates().is_empty(),
        "a file is not a duplicate of itself: {:?}",
        scanner.duplicates(),
    );
}

#[test]
fn the_same_tree_scans_the_same_way_twice() {
    let dir = tempfile::tempdir().expect("temp dir");
    for name in ["e", "c", "a", "d", "b"] {
        std::fs::write(dir.path().join(format!("{name}.php")), CLASS_PHP).unwrap();
    }

    let scan = || {
        let mut scanner = Scanner::new(None);
        scanner.scan_path(dir.path()).expect("scan");
        let duplicates: Vec<(String, String)> = scanner
            .duplicates()
            .iter()
            .map(|d| (d.kept.clone(), d.ignored.clone()))
            .collect();
        let entries: Vec<(String, String)> = scanner.into_iter().collect();
        (duplicates, entries)
    };

    // The winner comes from directory order rather than a sort, so re-scanning is
    // the only thing that shows a rebuild does not quietly move classmap entries.
    assert_eq!(scan(), scan());
}

#[test]
fn a_class_declared_once_is_not_a_duplicate() {
    let dir = tempfile::tempdir().expect("temp dir");

    std::fs::write(dir.path().join("a.php"), CLASS_PHP).unwrap();

    let mut scanner = Scanner::new(None);
    scanner.scan_path(dir.path()).expect("scan");

    assert!(scanner.duplicates().is_empty());
}
