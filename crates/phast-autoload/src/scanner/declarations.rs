//! Interpreting a token stream as type declarations.
//!
//! Given the tokens from [`Lexer`], this finds every `class` / `interface` /
//! `trait` / `enum` that is a *real* declaration and returns its fully-qualified name.
//!
//! The rule here is small: a type keyword declares a type unless the token just before
//! it was `::` or `->` (the `Foo::class` and `$obj->class` cases), and the token just after
//! it is a usable type name.

use super::lexer::{Lexer, Token};

use std::collections::HashSet;
use std::iter::Peekable;

/// Accumulates declared type names in source order, keeping only the first of each.
///
/// A file can declare the same class more than once across mutually exclusive
/// branches; statically that is one type, not a duplicate.
#[derive(Default)]
struct Declarations {
    /// Declared names, in the order they were first seen.
    names: Vec<String>,
    /// Names already recorded, so a repeat is dropped instead of appended.
    seen: HashSet<String>,
}

impl Declarations {
    /// Record `fqcn` unless an equal name was already recorded.
    fn record(&mut self, fqcn: String) {
        if self.seen.insert(fqcn.clone()) {
            self.names.push(fqcn);
        }
    }

    /// The recorded names, in first-declaration order.
    fn into_names(self) -> Vec<String> {
        self.names
    }
}

/// Every fully-qualified type name declared in `source`, each once,
/// in first-declaration order.
///
/// Class, interface, trait, and enum declarations are collected and qualified
/// with the enclosing namespace; member accesses (`Foo::class`) and repeated
/// declarations of the same name are not counted.
#[rustfmt::skip]
pub fn extract(source: &str) -> Vec<String> {
    let mut namespace = String::new();
    let mut declarations = Declarations::default();

    let mut tokens = Lexer::new(source).peekable();
    while let Some(token) = tokens.next() {
        match token {
            // The token after `::` or `->` names a member
            // (`Foo::class`, `$obj->class`), never a declaration, so drop it.
            Token::ScopeResolution | Token::ObjectOperator => {
                tokens.next();
            }

            // `namespace Foo\Bar;` sets the prefix later declarations qualify with.
            Token::Namespace => {
                if let Some(name) = peek(&mut tokens) {
                    namespace = name.to_owned();
                }
            }

            // A type keyword (`class`/`interface`/`trait`/`enum`)
            // followed by a usable name declares that type.
            Token::Type(_) => {
                if let Some(name) = peek(&mut tokens).filter(|name| is_type_name(name)) {
                    declarations.record(qualify(&namespace, name));
                }
            }

            // Every other token (names, punctuation, variables) declares nothing.
            _ => {}
        }
    }

    declarations.into_names()
}

/// The name the stream is about to yield, if its next token is a [`Token::Name`].
///
/// Looks without consuming, so the caller's own `next()` still returns it.
fn peek(tokens: &mut Peekable<Lexer>) -> Option<&str> {
    match tokens.peek() {
        Some(Token::Name(name)) => Some(name),
        _ => None,
    }
}

/// Join a namespace and a type name into a fully-qualified name.
fn qualify(namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        name.to_string()
    } else {
        format!("{namespace}\\{name}")
    }
}

/// Whether `word` can be a declared type name: a plain identifier, not a reserved word.
///
/// A real declaration keyword is always followed by its name. A reserved word in that
/// slot means the tokenizing was led astray (e.g. `new class extends Base`),
/// so rejecting it is a cheap guard on top of the member-access check.
fn is_type_name(word: &str) -> bool {
    is_identifier(word) && !is_reserved(word)
}

/// Whether `word` is a valid PHP identifier: a letter or underscore, then letters,
/// digits or underscores.
///
/// A declared type name never contains a namespace separator, so one appearing here
/// means the name slot held something other than a plain identifier.
#[rustfmt::skip]
fn is_identifier(word: &str) -> bool {
    let mut chars = word.chars();
    match chars.next() {
        Some(character)
          if character.is_alphabetic() ||
             character == '_' => chars.all(|character| character.is_alphanumeric() || character == '_'),
        _ => false,
    }
}

/// Whether `word` is a PHP reserved keyword (case-insensitive) that cannot name a type.
///
/// Not exhaustive: it only needs to cover words that can land in the name slot when
/// tokenizing goes wrong. Semi-reserved words that PHP *does* allow as class names
/// (`enum`, `match`, `fn`, `readonly`, ...) are deliberately excluded.
#[rustfmt::skip]
fn is_reserved(word: &str) -> bool {
    matches!(
        word.to_ascii_lowercase().as_str(),
        "array"     | "string"     | "int"    | "bool"      | "float"     | "void"   | "null" |
        "extends"   | "implements" | "use"    | "new"       | "return"    | "static"          |
        "self"      | "parent"     | "class"  | "interface" | "trait"     | "const"           |
        "abstract"  | "final"      | "public" | "private"   | "protected" |
        "namespace" | "function"   | "if"     | "else"      | "switch"    | "case"            |
        "instanceof",
    )
}
