//! Lexer: PHP source reduced to the tokens that matter for finding type declarations.
//!
//! Trivia (whitespace, comments, string and heredoc/nowdoc bodies) is consumed and
//! never emitted, so a consumer sees only structural tokens.
//!
//! The lexer's one job is to preserve the context that separates a real declaration
//! from a look-alike. A `$class` variable, a `Foo::class` constant, and a
//! `$obj->class` access all contain the word `class`, but only a bare `class` keyword
//! declares a type. Emitting [`Token::Variable`], [`Token::ScopeResolution`], and
//! [`Token::ObjectOperator`] as distinct tokens is what lets the declaration pass
//! discard the token after `::` or `->` without tracking any history of its own.

/// A significant PHP token.
///
/// Trivia is never represented: the lexer skips it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// A type-declaration keyword: `class`, `interface`, `trait`, or `enum`.
    Type(TypeKeyword),
    /// The `namespace` keyword.
    Namespace,
    /// An identifier or qualified name, e.g. `Baz` or `Foo\Bar`.
    Name(String),
    /// A `$foo` variable. Only that it *is* a variable matters, not its name, so a
    /// `$class` can never be read as the `class` keyword.
    Variable,
    /// `::`, which precedes the `::class` magic constant.
    ScopeResolution,
    /// `->`, which precedes a `$obj->class` property access.
    ObjectOperator,
    /// `;`, one terminator of a `namespace Foo;` statement.
    Semicolon,
    /// `{`, the other terminator of a `namespace Foo {` statement.
    OpenBrace,
}

/// The four keywords that introduce a type declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKeyword {
    Class,
    Interface,
    Trait,
    Enum,
}

/// Turns PHP source into [`Token`]s.
///
/// Construct with [`Lexer::new`] and iterate; each call to [`Iterator::next`] returns
/// the next significant token.
pub struct Lexer {
    /// The source as characters, so the cursor can index without UTF-8 boundary checks.
    chars: Vec<char>,
    /// Cursor into `chars`. May run past the end; [`Lexer::peek`] returns `None` there.
    position: usize,
}

impl Lexer {
    /// Create a lexer over `source`, positioned at the first PHP code.
    ///
    /// A PHP file starts in literal (HTML) mode, so the cursor begins past any leading
    /// inline text and its `<?php` open tag.
    pub fn new(source: &str) -> Self {
        let chars: Vec<char> = source.chars().collect();
        let position = php_code_start(&chars, 0);
        Self { chars, position }
    }

    /// The next significant token, or `None` at end of input.
    ///
    /// Reads one token kind per branch. A character the scanner does not care about
    /// (an operator, a bracket) is skipped and the search continues.
    fn next_token(&mut self) -> Option<Token> {
        loop {
            self.skip_trivia();

            let token = match self.peek(0)? {
                '$' => self.read_variable(),
                ':' if self.peek(1) == Some(':') => self.consume(2, Token::ScopeResolution),
                '-' if self.peek(1) == Some('>') => self.consume(2, Token::ObjectOperator),
                ';' => self.consume(1, Token::Semicolon),
                '{' => self.consume(1, Token::OpenBrace),
                character if is_name_start(character) => self.read_word(),
                _ => {
                    self.advance();
                    continue;
                }
            };

            return Some(token);
        }
    }

    /// Read a `$name` variable. The name is discarded; the token is just `Variable`.
    fn read_variable(&mut self) -> Token {
        self.advance(); // the `$`
        self.advance_while(is_name_continue);
        Token::Variable
    }

    /// Read a word and classify it as a keyword or a [`Token::Name`].
    #[rustfmt::skip]
    fn read_word(&mut self) -> Token {
        let start = self.position;
        self.advance_while(is_name_continue);

        classify(
            self.chars[start..self.position]
                .iter()
                .collect(),
        )
    }

    /// Consume every run of trivia at the cursor: whitespace, comments, string and
    /// heredoc/nowdoc bodies. Leaves the cursor on the next significant character.
    fn skip_trivia(&mut self) {
        loop {
            match self.peek(0) {
                Some(character) if character.is_whitespace() => self.advance(),
                Some('?') if self.peek(1) == Some('>') => self.skip_inline_html(),
                Some('#') if self.peek(1) == Some('[') => self.advance_by(2),
                Some('#') => self.skip_line_comment(),
                Some('/') if self.peek(1) == Some('/') => self.skip_line_comment(),
                Some('/') if self.peek(1) == Some('*') => self.skip_block_comment(),
                Some('\'') | Some('"') => self.skip_string(),
                Some('<') if self.starts_heredoc() => self.skip_heredoc(),
                _ => return,
            }
        }
    }

    /// A `?>` ends PHP; jump past the inline text to where PHP resumes: the next
    /// `<?php` open tag, or end of input. Shares [`php_code_start`] with construction.
    fn skip_inline_html(&mut self) {
        self.position = php_code_start(&self.chars, self.position);
    }

    /// Skip a `//` or `#` line comment: everything up to (but not including) the newline.
    fn skip_line_comment(&mut self) {
        self.advance_while(|character| character != '\n');
    }

    /// Skip a `/* ... */` block comment, including the closing `*/`.
    /// Runs to the end of input if the comment is never closed.
    #[rustfmt::skip]
    fn skip_block_comment(&mut self) {
        self.advance_by(2); // `/*`
        while self.peek(0).is_some() {
            if self.peek(0) == Some('*') &&
               self.peek(1) == Some('/')
            {
                self.advance_by(2);
                return;
            }
            self.advance();
        }
    }

    /// Skip a single- or double-quoted string, honoring backslash escapes.
    fn skip_string(&mut self) {
        let Some(quote) = self.peek(0) else {
            return;
        };

        self.advance(); // opening quote
        while let Some(character) = self.peek(0) {
            match character {
                '\\' => self.advance_by(2), // escape: skip `\` and the next char
                character if character == quote => {
                    self.advance();
                    return;
                }
                _ => self.advance(),
            }
        }
    }

    /// Whether the cursor is at a heredoc/nowdoc opener (`<<<`).
    #[rustfmt::skip]
    fn starts_heredoc(&self) -> bool {
        self.peek(1) == Some('<') &&
        self.peek(2) == Some('<')
    }

    /// Skip a heredoc/nowdoc: `<<<LABEL`, `<<<"LABEL"`, or `<<<'LABEL'`, up
    /// to the closing label. The closing label sits at the start of a line
    /// (indentation allowed, PHP 7.3+) and is followed by a non-identifier character.
    #[rustfmt::skip]
    fn skip_heredoc(&mut self) {
        self.advance_by(3); // `<<<`
        self.advance_while(|character| character == ' ' || character == '\t');

        if matches!(self.peek(0),
            Some('"') |
            Some('\'')
        ) {
            self.advance(); // opening quote of `<<<"LABEL"` / `<<<'LABEL'`
        }

        let start = self.position;
        self.advance_while(is_name_continue);

        let label: Vec<char> = self
            .chars[start..self.position]
            .to_vec();
        // not a real opener; carry on from here
        if label.is_empty() {
            return;
        }

        // Scan the body one line at a time: the heredoc ends at the first line
        // that is exactly the label, after optional indentation, at a token boundary.
        while self.peek(0).is_some() {
            self.advance_while(|character| character != '\n');
            if self.peek(0).is_none() {
                // unterminated: consumed to end of input
                return;
            }

            self.advance(); // the newline
            self.advance_while(|character| character == ' ' || character == '\t');

            if self.at_label(&label) {
                self.advance_by(label.len());
                return;
            }
        }
    }

    /// Whether the heredoc closing `label` is at the cursor, ending at a token
    /// boundary, so `EOT` closes the block but `EOTX` does not.
    fn at_label(&self, label: &[char]) -> bool {
        let label_matches = label
            .iter()
            .enumerate()
            .all(|(offset, &expected)| self.peek(offset) == Some(expected));

        let boundary_after = match self.peek(label.len()) {
            Some(character) => !is_name_continue(character),
            None => true,
        };

        label_matches && boundary_after
    }

    /// The character `offset` positions ahead of the cursor, if any.
    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.position + offset).copied()
    }

    /// Step the cursor forward one character.
    fn advance(&mut self) {
        self.advance_by(1);
    }

    /// Step the cursor forward `count` characters, past a fixed-width marker.
    fn advance_by(&mut self, count: usize) {
        self.position += count;
    }

    /// Advance the cursor while `predicate` holds.
    fn advance_while(&mut self, predicate: impl Fn(char) -> bool) {
        while matches!(self.peek(0), Some(character) if predicate(character)) {
            self.advance();
        }
    }

    /// Advance past a fixed-width token and return it.
    fn consume(&mut self, count: usize, token: Token) -> Token {
        self.advance_by(count);
        token
    }
}

impl Iterator for Lexer {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        self.next_token()
    }
}

/// Classify a word as a keyword or a plain name. A relative name like `namespace\Foo`
/// arrives here as one backslash-joined word, so it becomes a [`Token::Name`], not the
/// `namespace` keyword.
#[rustfmt::skip]
fn classify(word: String) -> Token {
    match word.as_str() {
        "class"     => Token::Type(TypeKeyword::Class),
        "interface" => Token::Type(TypeKeyword::Interface),
        "trait"     => Token::Type(TypeKeyword::Trait),
        "enum"      => Token::Type(TypeKeyword::Enum),
        "namespace" => Token::Namespace,
        _           => Token::Name(word),
    }
}

/// Whether `character` can start a name: a letter, underscore, or leading namespace
/// separator.
fn is_name_start(character: char) -> bool {
    character.is_alphabetic() || character == '_' || character == '\\'
}

/// Whether `character` can continue a name: letters, digits, underscores, and namespace
/// separators.
fn is_name_continue(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '\\'
}

/// The offset where PHP code resumes at or after `from`: just past the next
/// `<?php` / `<?=` / `<?` open tag, skipping any inline text (HTML) before it.
/// Returns `chars.len()` if there is no further open tag.
#[rustfmt::skip]
fn php_code_start(chars: &[char], from: usize) -> usize {
    let Some(offset) = chars[from..]
        .windows(2)
        .position(|pair| matches!(pair, ['<', '?'])) else
    {
        return chars.len();
    };

    let after_open = from + offset + 2;
    if chars[after_open..].starts_with(&['p', 'h', 'p']) {
        // `<?php`
        after_open + 3
    } else if chars.get(after_open) == Some(&'=') {
         // `<?=`
        after_open + 1
    } else {
        // short `<?`
        after_open
    }
}
