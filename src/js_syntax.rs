//! A small JavaScript front end for the runtime source and emitted output: a
//! lexer, a scope analysis that resolves every identifier to its declaration,
//! short names for locals, and a writer that joins tokens with the fewest
//! separators.
//!
//! [`crate::runtime_manifest`] uses the lexer and [`analyze`] to check unit
//! dependencies, and terse emission (docs/advanced-optimization.md, Phase 3)
//! uses [`resolve`], [`Resolution::local_names`] and [`TerseWriter`]. The
//! module has no dependencies inside the crate, so `build.rs` includes it as
//! well and strips and renames the runtime units at build time.

// Local renaming and the unit writer serve `build.rs` and the tests that
// check its output; the crate itself uses the rest.
#![cfg_attr(not(test), allow(dead_code))]

use std::ops::Range;

// ---------------------------------------------------------------------------
// Lexer

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Name,
    Private,
    Punct,
    Number,
    Str,
    Template,
    TemplateHead,
    TemplateMiddle,
    TemplateTail,
    Regex,
}

#[derive(Clone, Copy, Debug)]
pub struct Token {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    /// Whether a line terminator (or a comment holding one) precedes the token.
    pub newline_before: bool,
}

impl Token {
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
}

#[derive(Debug)]
pub struct LexError {
    pub offset: usize,
    pub message: &'static str,
}

const PUNCTUATORS: &[&str] = &[
    ">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "&&=", "||=", "??=", "=>", "==", "!=",
    "<=", ">=", "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=",
    "**", "<<", ">>", "{", "}", "(", ")", "[", "]", ";", ",", "<", ">", "+", "-", "*", "/", "%",
    "&", "|", "^", "!", "~", "?", ":", "=", ".", "@",
];

pub fn identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' || byte >= 0x80
}

pub fn identifier_part(byte: u8) -> bool {
    identifier_start(byte) || byte.is_ascii_digit()
}

/// Whether a `/` after `previous` starts a regular expression literal.
/// `closed_control` tells whether the last `)` closed an `if`, `while`,
/// `for` or `with` head, after which a statement (and so a regex) starts.
fn regex_allowed(source: &str, previous: Option<&Token>, closed_control: bool) -> bool {
    let Some(previous) = previous else {
        return true;
    };
    if previous.kind == Kind::Punct && previous.text(source) == ")" {
        return closed_control;
    }
    match previous.kind {
        Kind::Name => matches!(
            previous.text(source),
            "return"
                | "typeof"
                | "instanceof"
                | "in"
                | "of"
                | "new"
                | "delete"
                | "void"
                | "throw"
                | "case"
                | "do"
                | "else"
                | "yield"
                | "await"
        ),
        Kind::Punct => !matches!(previous.text(source), ")" | "]" | "}" | "++" | "--"),
        Kind::TemplateHead | Kind::TemplateMiddle => true,
        _ => false,
    }
}

/// Tokens of a unit body, which must close every bracket it opens.
pub fn lex(source: &str, range: Range<usize>) -> Result<Vec<Token>, LexError> {
    lex_section(source, range, true)
}

/// Tokens of `range`. Unbalanced sections (the prologue opens the factory
/// closure and the export section closes it) skip the brace checks.
pub fn lex_section(
    source: &str,
    range: Range<usize>,
    balanced: bool,
) -> Result<Vec<Token>, LexError> {
    let bytes = source.as_bytes();
    let end = range.end;
    let at = |i: usize| if i < end { bytes[i] } else { 0 };
    let mut i = range.start;
    let mut tokens: Vec<Token> = Vec::new();
    // One entry per open `{`: true when it is a template substitution.
    let mut braces: Vec<bool> = Vec::new();
    // One entry per open `(`: true when it opens a control statement head.
    let mut parens: Vec<bool> = Vec::new();
    let mut closed_control = false;
    let mut newline = false;
    // Scans template characters after a backtick or a substitution's `}`.
    let template = |mut i: usize| -> Result<(usize, bool), LexError> {
        loop {
            match at(i) {
                0 if i >= end => {
                    return Err(LexError {
                        offset: i,
                        message: "unterminated template literal",
                    });
                }
                b'\\' => i += 2,
                b'`' => return Ok((i + 1, true)),
                b'$' if at(i + 1) == b'{' => return Ok((i + 2, false)),
                _ => i += 1,
            }
        }
    };
    while i < end {
        let byte = bytes[i];
        match byte {
            b'\n' | b'\r' => {
                newline = true;
                i += 1;
                continue;
            }
            b' ' | b'\t' | 0x0b | 0x0c => {
                i += 1;
                continue;
            }
            b'/' if at(i + 1) == b'/' => {
                while i < end && !matches!(bytes[i], b'\n' | b'\r') {
                    i += 1;
                }
                continue;
            }
            b'/' if at(i + 1) == b'*' => {
                let Some(close) = source[i + 2..end].find("*/") else {
                    return Err(LexError {
                        offset: i,
                        message: "unterminated block comment",
                    });
                };
                newline |= source[i..i + 2 + close].contains(['\n', '\r']);
                i += close + 4;
                continue;
            }
            _ => {}
        }
        let start = i;
        let kind = match byte {
            b'`' => {
                let (next, ended) = template(i + 1)?;
                i = next;
                if ended {
                    Kind::Template
                } else {
                    braces.push(true);
                    Kind::TemplateHead
                }
            }
            b'}' if braces.last() == Some(&true) => {
                braces.pop();
                let (next, ended) = template(i + 1)?;
                i = next;
                if ended {
                    Kind::TemplateTail
                } else {
                    braces.push(true);
                    Kind::TemplateMiddle
                }
            }
            b'"' | b'\'' => {
                i += 1;
                loop {
                    match at(i) {
                        b'\\' if at(i + 1) == b'\r' && at(i + 2) == b'\n' => i += 3,
                        b'\\' => i += 2,
                        b'\n' | b'\r' | 0 => {
                            return Err(LexError {
                                offset: start,
                                message: "unterminated string literal",
                            });
                        }
                        quote if quote == byte => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                Kind::Str
            }
            b'0'..=b'9' | b'.' if byte != b'.' || at(i + 1).is_ascii_digit() => {
                let radix =
                    byte == b'0' && matches!(at(i + 1), b'x' | b'X' | b'o' | b'O' | b'b' | b'B');
                i += 1;
                // Digits, separators, a fraction, an exponent and its sign.
                while at(i).is_ascii_alphanumeric()
                    || matches!(at(i), b'_' | b'.')
                    || (matches!(at(i), b'+' | b'-') && !radix && matches!(at(i - 1), b'e' | b'E'))
                {
                    i += 1;
                }
                Kind::Number
            }
            b'#' if identifier_start(at(i + 1)) => {
                i += 1;
                while identifier_part(at(i)) {
                    i += 1;
                }
                Kind::Private
            }
            _ if identifier_start(byte) => {
                while identifier_part(at(i)) {
                    i += 1;
                }
                Kind::Name
            }
            b'/' if regex_allowed(source, tokens.last(), closed_control) => {
                i += 1;
                let mut class = false;
                loop {
                    match at(i) {
                        b'\n' | b'\r' | 0 => {
                            return Err(LexError {
                                offset: start,
                                message: "unterminated regular expression literal",
                            });
                        }
                        b'\\' => i += 2,
                        b'[' => {
                            class = true;
                            i += 1;
                        }
                        b']' => {
                            class = false;
                            i += 1;
                        }
                        b'/' if !class => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                while identifier_part(at(i)) {
                    i += 1;
                }
                Kind::Regex
            }
            _ => {
                let rest = &source[i..end];
                let Some(punctuator) = PUNCTUATORS.iter().find(|p| {
                    rest.starts_with(**p) && !(**p == "?." && at(i + 2).is_ascii_digit())
                }) else {
                    return Err(LexError {
                        offset: i,
                        message: "unexpected character",
                    });
                };
                i += punctuator.len();
                match *punctuator {
                    "(" => parens.push(tokens.last().is_some_and(|t| {
                        t.kind == Kind::Name
                            && matches!(t.text(source), "if" | "while" | "for" | "with")
                    })),
                    ")" => closed_control = parens.pop().unwrap_or(false),
                    "{" => braces.push(false),
                    "}" if braces.pop().is_none() && balanced => {
                        return Err(LexError {
                            offset: start,
                            message: "unbalanced `}`",
                        });
                    }
                    _ => {}
                }
                Kind::Punct
            }
        };
        if i > end {
            return Err(LexError {
                offset: start,
                message: "token runs past the end of its unit",
            });
        }
        tokens.push(Token {
            kind,
            start,
            end: i,
            newline_before: newline,
        });
        newline = false;
    }
    if braces.contains(&true) || (balanced && !braces.is_empty()) {
        return Err(LexError {
            offset: end,
            message: "unclosed `{` or template literal",
        });
    }
    Ok(tokens)
}

// ---------------------------------------------------------------------------
// Scope analysis

/// What [`analyze`] finds in a unit body.
pub struct Facts<'a> {
    /// Top-level bindings, in source order.
    pub declares: Vec<&'a str>,
    /// Free identifiers, sorted and without duplicates.
    pub references: Vec<&'a str>,
    /// The references called directly by code run at load time.
    pub load_time_calls: Vec<&'a str>,
    /// Whether some top-level statement is not a declaration.
    pub has_effect_statements: bool,
}

struct Scope<'a> {
    parent: usize,
    /// Function scopes receive `var` declarations; scope 0 is the unit's top level.
    function: bool,
    names: Vec<&'a str>,
}

/// The parent of the top-level scope.
pub const NO_SCOPE: usize = usize::MAX;

/// Reserved words and literals that are never identifier references.
pub fn reserved(word: &str) -> bool {
    matches!(
        word,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "let"
            | "new"
            | "null"
            | "return"
            | "static"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
            | "await"
            | "implements"
            | "interface"
            | "package"
            | "private"
            | "protected"
            | "public"
    )
}

/// Words that cannot continue an expression; meeting one ends it (after ASI).
fn ends_expression(word: &str) -> bool {
    matches!(
        word,
        "var"
            | "let"
            | "const"
            | "if"
            | "for"
            | "while"
            | "do"
            | "return"
            | "throw"
            | "try"
            | "switch"
            | "break"
            | "continue"
            | "debugger"
            | "case"
            | "default"
            | "else"
            | "catch"
            | "finally"
            | "with"
            | "import"
            | "export"
    )
}

struct Parser<'t, 'a> {
    source: &'a str,
    tokens: &'t [Token],
    /// For each bracket token, the index of its partner.
    partners: Vec<usize>,
    pos: usize,
    scopes: Vec<Scope<'a>>,
    references: Vec<Reference<'a>>,
    /// Each token that declares a name: the token, the scope it declares the
    /// name in, the scope it appears in (a `var` in a block declares in the
    /// function) and whether it is the key of a shorthand pattern property
    /// (`{a} = b`).
    declarations: Vec<(usize, usize, usize, bool)>,
    /// Each `var`, `let` or `const` statement ended by a `;`: the keyword's
    /// token and the `;`'s.
    statements: Vec<(usize, usize)>,
    error: Option<(usize, String)>,
}

/// An identifier reference: its scope, whether it is the callee of a call, its
/// token and whether it is a shorthand property (`{a}`).
struct Reference<'a> {
    name: &'a str,
    scope: usize,
    call: bool,
    token: usize,
    shorthand: bool,
}

#[derive(Clone, Copy)]
struct Stops {
    comma: bool,
    /// The left-hand side of a `for` head ends at `of` or `in`.
    for_head: bool,
}

const SEQUENCE: Stops = Stops {
    comma: false,
    for_head: false,
};
const ASSIGNMENT: Stops = Stops {
    comma: true,
    for_head: false,
};

/// The bracket partners of `tokens` and the scopes, declarations and
/// references of a parse of them as a sequence of statements; and whether some
/// top-level statement is not a declaration.
fn parse<'t, 'a>(
    source: &'a str,
    tokens: &'t [Token],
) -> Result<(Parser<'t, 'a>, bool), (usize, String)> {
    let mut partners = vec![usize::MAX; tokens.len()];
    let mut open = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != Kind::Punct {
            continue;
        }
        match token.text(source) {
            "(" | "[" | "{" => open.push(index),
            close @ (")" | "]" | "}") => {
                let expected = match close {
                    ")" => "(",
                    "]" => "[",
                    _ => "{",
                };
                match open.pop() {
                    Some(opener) if tokens[opener].text(source) == expected => {
                        partners[opener] = index;
                        partners[index] = opener;
                    }
                    _ => return Err((token.start, format!("unbalanced `{close}`"))),
                }
            }
            _ => {}
        }
    }
    if let Some(&opener) = open.last() {
        return Err((
            tokens[opener].start,
            format!("unclosed `{}`", tokens[opener].text(source)),
        ));
    }
    let mut parser = Parser {
        source,
        tokens,
        partners,
        pos: 0,
        scopes: vec![Scope {
            parent: NO_SCOPE,
            function: true,
            names: Vec::new(),
        }],
        references: Vec::new(),
        declarations: Vec::new(),
        statements: Vec::new(),
        error: None,
    };
    let mut has_effect_statements = false;
    while parser.pos < tokens.len() && parser.error.is_none() {
        let declaration = match parser.word() {
            Some("var" | "let" | "const" | "function" | "class") => true,
            Some("async") => parser.word_at(parser.pos + 1) == Some("function"),
            _ => parser.punct() == Some(";"),
        };
        has_effect_statements |= !declaration;
        parser.statement_progressing(0);
    }
    match parser.error.take() {
        Some(error) => Err(error),
        None => Ok((parser, has_effect_statements)),
    }
}

/// The top-level bindings and free identifiers of a unit body.
pub fn analyze<'a>(source: &'a str, tokens: &[Token]) -> Result<Facts<'a>, (usize, String)> {
    let (parser, has_effect_statements) = parse(source, tokens)?;
    let declares = parser.scopes[0].names.clone();
    let mut references = Vec::new();
    let mut load_time_calls = Vec::new();
    for &Reference {
        name, scope, call, ..
    } in &parser.references
    {
        let mut current = scope;
        let mut load_time = true;
        let local = loop {
            if current == 0 {
                break parser.scopes[0].names.contains(&name);
            }
            if parser.scopes[current].names.contains(&name) {
                break true;
            }
            load_time &= !parser.scopes[current].function;
            current = parser.scopes[current].parent;
        };
        if !local {
            references.push(name);
            if load_time && call {
                load_time_calls.push(name);
            }
        }
    }
    references.sort_unstable();
    references.dedup();
    load_time_calls.sort_unstable();
    load_time_calls.dedup();
    Ok(Facts {
        declares,
        references,
        load_time_calls,
        has_effect_statements,
    })
}

impl<'a> Parser<'_, 'a> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn word_at(&self, index: usize) -> Option<&'a str> {
        self.tokens
            .get(index)
            .filter(|t| t.kind == Kind::Name)
            .map(|t| t.text(self.source))
    }

    fn word(&self) -> Option<&'a str> {
        self.word_at(self.pos)
    }

    fn punct_at(&self, index: usize) -> Option<&'a str> {
        self.tokens
            .get(index)
            .filter(|t| t.kind == Kind::Punct)
            .map(|t| t.text(self.source))
    }

    fn punct(&self) -> Option<&'a str> {
        self.punct_at(self.pos)
    }

    fn at_punct(&self, text: &str) -> bool {
        self.punct() == Some(text)
    }

    fn at_word(&self, text: &str) -> bool {
        self.word() == Some(text)
    }

    fn fail(&mut self, message: impl Into<String>) {
        if self.error.is_none() {
            let offset = self.peek().or(self.tokens.last()).map_or(0, |t| t.start);
            self.error = Some((offset, message.into()));
        }
        self.pos = self.tokens.len();
    }

    fn expect(&mut self, text: &str) {
        if self.at_punct(text) {
            self.pos += 1;
        } else {
            let found = self.peek().map_or("the end of the unit".to_string(), |t| {
                format!("`{}`", t.text(self.source))
            });
            self.fail(format!("expected `{text}`, found {found}"));
        }
    }

    fn eat(&mut self, text: &str) -> bool {
        let found = self.at_punct(text);
        self.pos += usize::from(found);
        found
    }

    fn scope(&mut self, parent: usize, function: bool) -> usize {
        self.scopes.push(Scope {
            parent,
            function,
            names: Vec::new(),
        });
        self.scopes.len() - 1
    }

    fn declare(&mut self, scope: usize, name: &'a str) {
        if !self.scopes[scope].names.contains(&name) {
            self.scopes[scope].names.push(name);
        }
    }

    /// Declares the name of the token at `token`, which appears in `lexical`,
    /// in `scope`.
    fn declare_token(&mut self, scope: usize, lexical: usize, token: usize, shorthand: bool) {
        self.declare(scope, self.tokens[token].text(self.source));
        self.declarations.push((token, scope, lexical, shorthand));
    }

    fn function_scope(&self, mut scope: usize) -> usize {
        while !self.scopes[scope].function {
            scope = self.scopes[scope].parent;
        }
        scope
    }

    /// Records the identifier at the cursor as a reference resolved in `scope`.
    fn reference(&mut self, name: &'a str, scope: usize) {
        let call = self.word() == Some(name) && self.punct_at(self.pos + 1) == Some("(");
        self.references.push(Reference {
            name,
            scope,
            call,
            token: self.pos,
            shorthand: false,
        });
    }

    /// The single-token property key at the cursor, if any.
    fn key_start(&self, index: usize) -> bool {
        self.tokens.get(index).is_some_and(|t| {
            matches!(
                t.kind,
                Kind::Name | Kind::Str | Kind::Number | Kind::Private
            ) || (t.kind == Kind::Punct && matches!(t.text(self.source), "[" | "*"))
        })
    }

    /// Whether the `(` at `index` opens arrow function parameters.
    fn arrow_parameters_at(&self, index: usize) -> bool {
        self.punct_at(index) == Some("(") && self.punct_at(self.partners[index] + 1) == Some("=>")
    }

    fn statements_until_brace(&mut self, scope: usize) {
        while self.pos < self.tokens.len() && !self.at_punct("}") {
            self.statement_progressing(scope);
        }
        self.expect("}");
    }

    fn statement_progressing(&mut self, scope: usize) {
        let start = self.pos;
        self.statement(scope);
        if self.pos == start {
            let text = self.peek().map_or("", |t| t.text(self.source)).to_string();
            self.fail(format!("unexpected `{text}`"));
        }
    }

    fn statement(&mut self, scope: usize) {
        if self.at_punct("{") {
            self.pos += 1;
            let block = self.scope(scope, false);
            self.statements_until_brace(block);
            return;
        }
        if self.eat(";") {
            return;
        }
        let Some(word) = self.word() else {
            self.expression(scope, SEQUENCE);
            self.eat(";");
            return;
        };
        match word {
            "var" | "let" | "const" => {
                let keyword = self.pos;
                self.pos += 1;
                let target = if word == "var" {
                    self.function_scope(scope)
                } else {
                    scope
                };
                self.declarations(target, scope, false);
                if self.eat(";") {
                    self.statements.push((keyword, self.pos - 1));
                }
            }
            "function" => self.function_declaration(scope),
            "async" if self.word_at(self.pos + 1) == Some("function") => {
                self.pos += 1;
                self.function_declaration(scope);
            }
            "class" => {
                self.pos += 1;
                match self.word() {
                    Some(name) if !reserved(name) => {
                        self.pos += 1;
                        // The body's binding of the name is the declaration's.
                        self.declare_token(scope, scope, self.pos - 1, false);
                        self.class_tail(scope, None);
                    }
                    _ => self.fail("expected a class name"),
                }
            }
            "if" => {
                self.pos += 1;
                self.parenthesized(scope);
                self.statement_progressing(scope);
                if self.at_word("else") {
                    self.pos += 1;
                    self.statement_progressing(scope);
                }
            }
            "for" => self.for_statement(scope),
            "while" => {
                self.pos += 1;
                self.parenthesized(scope);
                self.statement_progressing(scope);
            }
            "do" => {
                self.pos += 1;
                self.statement_progressing(scope);
                if !self.at_word("while") {
                    return self.fail("expected `while` after a `do` body");
                }
                self.pos += 1;
                self.parenthesized(scope);
                self.eat(";");
            }
            "switch" => {
                self.pos += 1;
                self.parenthesized(scope);
                self.expect("{");
                let block = self.scope(scope, false);
                while self.pos < self.tokens.len() && !self.at_punct("}") {
                    if self.at_word("case") {
                        self.pos += 1;
                        self.expression(block, SEQUENCE);
                        self.expect(":");
                    } else if self.at_word("default") {
                        self.pos += 1;
                        self.expect(":");
                    } else {
                        self.statement_progressing(block);
                    }
                }
                self.expect("}");
            }
            "try" => {
                self.pos += 1;
                self.block(scope);
                if self.at_word("catch") {
                    self.pos += 1;
                    let catch = self.scope(scope, false);
                    if self.eat("(") {
                        self.binding(catch, catch);
                        self.expect(")");
                    }
                    // The parameter and the body's declarations share a scope:
                    // they cannot have the same name.
                    self.expect("{");
                    self.statements_until_brace(catch);
                }
                if self.at_word("finally") {
                    self.pos += 1;
                    self.block(scope);
                }
            }
            "return" | "throw" => {
                self.pos += 1;
                let ends = self.peek().is_none_or(|t| {
                    (word == "return" && t.newline_before)
                        || matches!(self.punct(), Some(";" | "}"))
                });
                if !ends {
                    self.expression(scope, SEQUENCE);
                }
                self.eat(";");
            }
            "break" | "continue" => {
                self.pos += 1;
                if self
                    .peek()
                    .is_some_and(|t| t.kind == Kind::Name && !t.newline_before)
                {
                    self.pos += 1;
                }
                self.eat(";");
            }
            "debugger" => {
                self.pos += 1;
                self.eat(";");
            }
            label if !reserved(label) && self.punct_at(self.pos + 1) == Some(":") => {
                self.pos += 2;
                self.statement_progressing(scope);
            }
            _ => {
                self.expression(scope, SEQUENCE);
                self.eat(";");
            }
        }
    }

    fn block(&mut self, scope: usize) {
        self.expect("{");
        let block = self.scope(scope, false);
        self.statements_until_brace(block);
    }

    fn parenthesized(&mut self, scope: usize) {
        self.expect("(");
        self.expression(scope, SEQUENCE);
        self.expect(")");
    }

    fn for_statement(&mut self, scope: usize) {
        self.pos += 1;
        if self.at_word("await") {
            self.pos += 1;
        }
        self.expect("(");
        let head = self.scope(scope, false);
        match self.word() {
            Some(word @ ("var" | "let" | "const")) => {
                self.pos += 1;
                let target = if word == "var" {
                    self.function_scope(head)
                } else {
                    head
                };
                self.declarations(target, head, true);
            }
            _ if self.at_punct(";") => {}
            _ => self.expression(
                head,
                Stops {
                    comma: false,
                    for_head: true,
                },
            ),
        }
        if self.at_word("of") || self.at_word("in") {
            self.pos += 1;
            self.expression(head, SEQUENCE);
        } else {
            self.expect(";");
            if !self.at_punct(";") {
                self.expression(head, SEQUENCE);
            }
            self.expect(";");
            if !self.at_punct(")") {
                self.expression(head, SEQUENCE);
            }
        }
        self.expect(")");
        self.statement_progressing(head);
    }

    /// Declarators after `var`/`let`/`const`: names go to `target`,
    /// initializers and defaults resolve in `scope`.
    fn declarations(&mut self, target: usize, scope: usize, for_head: bool) {
        loop {
            self.binding(target, scope);
            if self.eat("=") {
                self.expression(scope, ASSIGNMENT);
            }
            if for_head && (self.at_word("of") || self.at_word("in")) {
                return;
            }
            if !self.eat(",") {
                return;
            }
        }
    }

    /// A binding identifier or destructuring pattern.
    fn binding(&mut self, target: usize, scope: usize) {
        match self.peek().map(|t| (t.kind, t.text(self.source))) {
            Some((Kind::Name, name)) if !reserved(name) => {
                self.pos += 1;
                self.declare_token(target, scope, self.pos - 1, false);
            }
            Some((Kind::Punct, "[")) => {
                self.pos += 1;
                while self.pos < self.tokens.len() && !self.at_punct("]") {
                    if self.eat(",") {
                        continue;
                    }
                    self.eat("...");
                    self.binding(target, scope);
                    if self.eat("=") {
                        self.expression(scope, ASSIGNMENT);
                    }
                    if !self.at_punct("]") {
                        self.expect(",");
                    }
                }
                self.expect("]");
            }
            Some((Kind::Punct, "{")) => {
                self.pos += 1;
                while self.pos < self.tokens.len() && !self.at_punct("}") {
                    if self.eat("...") {
                        self.binding(target, scope);
                    } else if self.eat("[") {
                        self.expression(scope, SEQUENCE);
                        self.expect("]");
                        self.expect(":");
                        self.binding(target, scope);
                    } else {
                        let Some(key) = self.peek().copied() else {
                            break;
                        };
                        if !matches!(key.kind, Kind::Name | Kind::Str | Kind::Number) {
                            return self.fail("expected a property name in an object pattern");
                        }
                        self.pos += 1;
                        if self.eat(":") {
                            self.binding(target, scope);
                        } else if key.kind == Kind::Name && !reserved(key.text(self.source)) {
                            self.declare_token(target, scope, self.pos - 1, true);
                        } else {
                            return self.fail("expected `:` after a non-identifier pattern key");
                        }
                    }
                    if self.eat("=") {
                        self.expression(scope, ASSIGNMENT);
                    }
                    if !self.at_punct("}") {
                        self.expect(",");
                    }
                }
                self.expect("}");
            }
            _ => self.fail("expected a binding name or pattern"),
        }
    }

    fn function_declaration(&mut self, scope: usize) {
        self.pos += 1; // function
        self.eat("*");
        match self.word() {
            Some(name) if !reserved(name) => {
                self.pos += 1;
                self.declare_token(scope, scope, self.pos - 1, false);
                self.function_rest(scope, None, true);
            }
            _ => self.fail("expected a function name"),
        }
    }

    /// Parameters and body, from the `(`. `own_name`, the token of a named
    /// function expression's name, binds it inside itself.
    fn function_rest(&mut self, outer: usize, own_name: Option<usize>, has_arguments: bool) {
        let function = self.scope(outer, true);
        if let Some(token) = own_name {
            self.declare_token(function, function, token, false);
        }
        if has_arguments {
            self.declare(function, "arguments");
        }
        self.parameters(function);
        self.expect("{");
        self.statements_until_brace(function);
    }

    fn parameters(&mut self, function: usize) {
        self.expect("(");
        while self.pos < self.tokens.len() && !self.at_punct(")") {
            self.eat("...");
            self.binding(function, function);
            if self.eat("=") {
                self.expression(function, ASSIGNMENT);
            }
            if !self.at_punct(")") {
                self.expect(",");
            }
        }
        self.expect(")");
    }

    /// An arrow function from its parameters (a name or a parenthesized list).
    fn arrow(&mut self, outer: usize) {
        let function = self.scope(outer, true);
        if self.at_punct("(") {
            self.parameters(function);
        } else {
            self.binding(function, function);
        }
        self.expect("=>");
        if self.eat("{") {
            self.statements_until_brace(function);
        } else {
            self.expression(function, ASSIGNMENT);
        }
    }

    /// A class from its heritage. `name`, the token of a class expression's
    /// name, binds it inside the class.
    fn class_tail(&mut self, scope: usize, name: Option<usize>) {
        let class = self.scope(scope, false);
        if let Some(token) = name {
            self.declare_token(class, class, token, false);
        }
        if self.at_word("extends") {
            self.pos += 1;
            // A left-hand-side expression; its end is the class body's `{`.
            while self.pos < self.tokens.len() && !self.at_punct("{") {
                if self.at_punct("(") || self.at_punct("[") {
                    let close = self.partners[self.pos];
                    self.pos += 1;
                    self.expression(scope, SEQUENCE);
                    if self.pos != close {
                        return self.fail("malformed class heritage");
                    }
                    self.pos += 1;
                } else if self.at_punct(".") || self.at_punct("?.") {
                    self.pos += 2;
                } else if let Some(word) = self.word().filter(|word| !reserved(word)) {
                    self.reference(word, scope);
                    self.pos += 1;
                } else {
                    self.pos += 1;
                }
            }
        }
        self.expect("{");
        while self.pos < self.tokens.len() && !self.at_punct("}") {
            if self.eat(";") {
                continue;
            }
            if self.at_word("static") && self.punct_at(self.pos + 1) == Some("{") {
                self.pos += 2;
                let block = self.scope(class, true);
                self.statements_until_brace(block);
                continue;
            }
            self.member_key(class);
            if self.at_punct("(") {
                self.function_rest(class, None, true);
            } else {
                if self.eat("=") {
                    let initializer = self.scope(class, true);
                    self.expression(initializer, ASSIGNMENT);
                }
                if !self.at_punct("}") {
                    self.expect(";");
                }
            }
        }
        self.expect("}");
    }

    /// Modifiers and the key of an object or class member. Returns the key
    /// when it is a plain identifier (a shorthand candidate).
    fn member_key(&mut self, scope: usize) -> Option<&'a str> {
        let mut modified = false;
        loop {
            match self.peek().map(|t| (t.kind, t.text(self.source))) {
                Some((Kind::Name, "get" | "set" | "async" | "static"))
                    if self.key_start(self.pos + 1) =>
                {
                    self.pos += 1;
                    modified = true;
                }
                Some((Kind::Punct, "*")) => {
                    self.pos += 1;
                    modified = true;
                }
                _ => break,
            }
        }
        match self.peek().copied() {
            Some(token) if token.kind == Kind::Punct && token.text(self.source) == "[" => {
                self.pos += 1;
                self.expression(scope, SEQUENCE);
                self.expect("]");
                None
            }
            Some(token)
                if matches!(
                    token.kind,
                    Kind::Name | Kind::Str | Kind::Number | Kind::Private
                ) =>
            {
                self.pos += 1;
                (token.kind == Kind::Name && !modified).then(|| token.text(self.source))
            }
            _ => {
                self.fail("expected a property name");
                None
            }
        }
    }

    fn object_literal(&mut self, scope: usize) {
        self.pos += 1; // {
        while self.pos < self.tokens.len() && !self.at_punct("}") {
            if self.eat("...") {
                self.expression(scope, ASSIGNMENT);
            } else {
                let shorthand = self.member_key(scope);
                if self.at_punct("(") {
                    self.function_rest(scope, None, true);
                } else if self.eat(":") {
                    self.expression(scope, ASSIGNMENT);
                } else {
                    match shorthand {
                        Some(name) if !reserved(name) => self.references.push(Reference {
                            name,
                            scope,
                            call: false,
                            token: self.pos - 1,
                            shorthand: true,
                        }),
                        _ => return self.fail("expected `:` or `(` after a property name"),
                    }
                    if self.eat("=") {
                        self.expression(scope, ASSIGNMENT);
                    }
                }
            }
            if !self.at_punct("}") {
                self.expect(",");
            }
        }
        self.expect("}");
    }

    /// Whether the token at `index` ends an operand, so that an operand on
    /// the next line starts a new statement (automatic semicolon insertion).
    fn ends_operand(&self, index: usize) -> bool {
        let token = &self.tokens[index];
        match token.kind {
            Kind::Name => {
                let word = token.text(self.source);
                !reserved(word) || matches!(word, "this" | "super" | "null" | "true" | "false")
            }
            Kind::Punct => matches!(token.text(self.source), ")" | "]" | "}"),
            Kind::TemplateHead | Kind::TemplateMiddle => false,
            _ => true,
        }
    }

    /// Scans an expression, recording references and nested scopes. Stops
    /// before a closing bracket, `;`, a `:` that closes no `?`, a statement
    /// keyword or an operand on a new line after an operand (automatic
    /// semicolon insertion) and, with `stops.comma`, `,`.
    fn expression(&mut self, scope: usize, stops: Stops) {
        let mut conditionals = 0usize;
        let start = self.pos;
        while let Some(token) = self.peek().copied() {
            let text = token.text(self.source);
            let starts_operand = match token.kind {
                Kind::Name => !matches!(text, "in" | "instanceof"),
                Kind::Punct | Kind::TemplateMiddle | Kind::TemplateTail => false,
                _ => true,
            };
            if token.newline_before
                && starts_operand
                && self.pos > start
                && self.ends_operand(self.pos - 1)
            {
                return;
            }
            match token.kind {
                Kind::Punct => match text {
                    ")" | "]" | "}" | ";" => return,
                    "," if stops.comma => return,
                    ":" if conditionals == 0 => return,
                    ":" => {
                        conditionals -= 1;
                        self.pos += 1;
                    }
                    "?" => {
                        conditionals += 1;
                        self.pos += 1;
                    }
                    "." | "?." => {
                        self.pos += 1;
                        if self
                            .peek()
                            .is_some_and(|t| matches!(t.kind, Kind::Name | Kind::Private))
                        {
                            self.pos += 1;
                        }
                    }
                    "(" if self.arrow_parameters_at(self.pos) => self.arrow(scope),
                    "(" | "[" => {
                        let close = self.partners[self.pos];
                        self.pos += 1;
                        self.expression(scope, SEQUENCE);
                        if self.pos != close {
                            return self.fail(format!(
                                "unexpected `{}`",
                                self.peek().map_or("", |t| t.text(self.source))
                            ));
                        }
                        self.pos += 1;
                    }
                    "{" => self.object_literal(scope),
                    _ => self.pos += 1,
                },
                Kind::TemplateHead => {
                    self.pos += 1;
                    loop {
                        self.expression(scope, SEQUENCE);
                        match self.peek().map(|t| t.kind) {
                            Some(Kind::TemplateMiddle) => self.pos += 1,
                            Some(Kind::TemplateTail) => {
                                self.pos += 1;
                                break;
                            }
                            _ => return self.fail("malformed template literal substitution"),
                        }
                    }
                }
                Kind::TemplateMiddle | Kind::TemplateTail => return,
                Kind::Name => match text {
                    "function" => {
                        self.pos += 1;
                        self.eat("*");
                        let own = self.word().filter(|word| !reserved(word)).map(|_| self.pos);
                        self.pos += usize::from(own.is_some());
                        self.function_rest(scope, own, true);
                    }
                    "async"
                        if self.word_at(self.pos + 1) == Some("function")
                            && !self.tokens[self.pos + 1].newline_before =>
                    {
                        self.pos += 1;
                    }
                    "async"
                        if self.arrow_parameters_at(self.pos + 1)
                            || (self.word_at(self.pos + 1).is_some()
                                && self.punct_at(self.pos + 2) == Some("=>")) =>
                    {
                        self.pos += 1;
                        self.arrow(scope);
                    }
                    "class" => {
                        self.pos += 1;
                        let own = self
                            .word()
                            .filter(|word| !reserved(word) && *word != "extends")
                            .map(|_| self.pos);
                        self.pos += usize::from(own.is_some());
                        self.class_tail(scope, own);
                    }
                    "of" | "in" if stops.for_head => return,
                    word if ends_expression(word) => return,
                    word if reserved(word) => self.pos += 1,
                    _ if self.punct_at(self.pos + 1) == Some("=>") => self.arrow(scope),
                    word => {
                        self.reference(word, scope);
                        self.pos += 1;
                    }
                },
                _ => self.pos += 1,
            }
        }
    }
}

/// Global bindings of the ECMAScript specification that a unit may reference
/// without declaring them. Host globals (`console`, timers, ...) are absent.
pub const ECMASCRIPT_GLOBALS: &[&str] = &[
    "AggregateError",
    "Array",
    "ArrayBuffer",
    "Atomics",
    "BigInt",
    "BigInt64Array",
    "BigUint64Array",
    "Boolean",
    "DataView",
    "Date",
    "Error",
    "EvalError",
    "FinalizationRegistry",
    "Float16Array",
    "Float32Array",
    "Float64Array",
    "Function",
    "Infinity",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "Iterator",
    "JSON",
    "Map",
    "Math",
    "NaN",
    "Number",
    "Object",
    "Promise",
    "Proxy",
    "RangeError",
    "ReferenceError",
    "Reflect",
    "RegExp",
    "Set",
    "SharedArrayBuffer",
    "String",
    "Symbol",
    "SyntaxError",
    "TypeError",
    "URIError",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "WeakMap",
    "WeakRef",
    "WeakSet",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "eval",
    "globalThis",
    "isFinite",
    "isNaN",
    "parseFloat",
    "parseInt",
    "undefined",
];

// ---------------------------------------------------------------------------
// Resolution

/// A name a body declares.
#[derive(Clone, Debug)]
pub struct Declaration<'a> {
    pub name: &'a str,
    /// The scope it is declared in; 0 is the body's top level.
    pub scope: usize,
    /// The first token that declares it. `arguments` has none and keeps its
    /// name.
    pub token: Option<usize>,
}

/// What an identifier token declares or references ([`resolve`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Occurrence {
    /// The declaration, by index into [`Resolution::declarations`], or `None`
    /// for a free identifier: another unit's binding or a global.
    pub declaration: Option<usize>,
    /// The scope the token appears in.
    pub scope: usize,
    /// A shorthand property, `{a}`, of an object literal or pattern: renaming
    /// it writes the key, `{a: b}`.
    pub shorthand: bool,
}

/// The scopes of a body and what each of its identifier tokens names.
pub struct Resolution<'a> {
    /// Per scope, its parent; scope 0, the top level, has [`NO_SCOPE`]. A
    /// parent always precedes its children.
    pub parents: Vec<usize>,
    pub declarations: Vec<Declaration<'a>>,
    /// Per token: what it names, for tokens that declare or reference a binding.
    pub occurrences: Vec<Option<Occurrence>>,
    /// Each `var`, `let` or `const` statement ended by a `;`: the keyword's
    /// token and the `;`'s.
    pub statements: Vec<(usize, usize)>,
}

/// Resolves every identifier of a body to its declaration. Property names,
/// object keys, labels and keywords name nothing.
pub fn resolve<'a>(source: &'a str, tokens: &[Token]) -> Result<Resolution<'a>, (usize, String)> {
    let (parser, _) = parse(source, tokens)?;
    let mut declarations = Vec::new();
    let mut by_scope = Vec::with_capacity(parser.scopes.len());
    for (index, scope) in parser.scopes.iter().enumerate() {
        by_scope.push(declarations.len());
        declarations.extend(scope.names.iter().map(|&name| Declaration {
            name,
            scope: index,
            token: None,
        }));
    }
    let lookup = |scope: usize, name: &str| {
        parser.scopes[scope]
            .names
            .iter()
            .position(|&declared| declared == name)
            .map(|position| by_scope[scope] + position)
    };
    let mut occurrences = vec![None; tokens.len()];
    for &(token, scope, lexical, shorthand) in &parser.declarations {
        let declaration = lookup(scope, tokens[token].text(source)).expect("a declared name");
        declarations[declaration].token.get_or_insert(token);
        occurrences[token] = Some(Occurrence {
            declaration: Some(declaration),
            scope: lexical,
            shorthand,
        });
    }
    for reference in &parser.references {
        let mut current = reference.scope;
        let declaration = loop {
            if let Some(declaration) = lookup(current, reference.name) {
                break Some(declaration);
            }
            if current == 0 {
                break None;
            }
            current = parser.scopes[current].parent;
        };
        occurrences[reference.token] = Some(Occurrence {
            declaration,
            scope: reference.scope,
            shorthand: reference.shorthand,
        });
    }
    Ok(Resolution {
        parents: parser.scopes.iter().map(|scope| scope.parent).collect(),
        declarations,
        occurrences,
        statements: parser.statements,
    })
}

/// How a token changes when adjacent declarations join
/// ([`joined_declarations`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    /// Written as it is.
    Keep,
    /// The `;` ending a declaration that the next continues: written `,`.
    Comma,
    /// The keyword of a declaration that continues the one before it.
    Drop,
}

/// Joins each `let` or `const` statement with the one right after it of the
/// same kind: `const a = 1; const b = 2;` is `const a = 1, b = 2;`, which
/// declares, initializes and leaves the bindings in their temporal dead
/// zones in the same order. (Two declaration statements that touch are in
/// one statement list; neither can be a statement's body.)
pub fn joined_declarations(
    source: &str,
    tokens: &[Token],
    statements: &[(usize, usize)],
) -> Vec<Join> {
    let mut joins = vec![Join::Keep; tokens.len()];
    let mut statements = statements.to_vec();
    statements.sort_unstable();
    for pair in statements.windows(2) {
        let [(first, semicolon), (second, _)] = [pair[0], pair[1]];
        let keyword = tokens[first].text(source);
        if second == semicolon + 1 && keyword != "var" && tokens[second].text(source) == keyword {
            joins[semicolon] = Join::Comma;
            joins[second] = Join::Drop;
        }
    }
    joins
}

impl Resolution<'_> {
    /// Short names for every declaration below the top level, by declaration
    /// index; `None` for top-level and token-less declarations and for those
    /// that `keep` holds, which keep their names. Scopes are named
    /// outermost first, each from the start of the [`NameGenerator`]
    /// sequence, skipping the names of outer declarations that its code reads
    /// or declares (a `var` in a block), so that no new name captures another,
    /// and skipping `taken` names (globals). A scope's own declarations get
    /// distinct names in declaration order, so parameters read `a, b, c`.
    pub fn local_names(
        &self,
        keep: impl Fn(&Declaration) -> bool,
        taken: impl Fn(&str) -> bool,
    ) -> Vec<Option<String>> {
        let mut crossing: Vec<Vec<usize>> = vec![Vec::new(); self.parents.len()];
        for occurrence in self.occurrences.iter().flatten() {
            let Some(declaration) = occurrence.declaration else {
                continue;
            };
            let target = self.declarations[declaration].scope;
            if target == 0 {
                continue;
            }
            let mut current = occurrence.scope;
            while current != target && current != NO_SCOPE {
                crossing[current].push(declaration);
                current = self.parents[current];
            }
        }
        let mut names: Vec<Option<String>> = vec![None; self.declarations.len()];
        let mut start = 0;
        for (scope, crossing) in crossing.iter().enumerate() {
            let end = start
                + self.declarations[start..]
                    .iter()
                    .take_while(|declaration| declaration.scope == scope)
                    .count();
            if scope != 0 {
                let mut used = crossing
                    .iter()
                    .map(|&outer| {
                        names[outer]
                            .clone()
                            .unwrap_or_else(|| self.declarations[outer].name.to_owned())
                    })
                    .collect::<Vec<_>>();
                let kept =
                    |declaration: &Declaration| declaration.token.is_none() || keep(declaration);
                used.extend(
                    self.declarations[start..end]
                        .iter()
                        .filter(|declaration| kept(declaration))
                        .map(|declaration| declaration.name.to_owned()),
                );
                let mut generator = NameGenerator::default();
                for (index, declaration) in self.declarations[start..end].iter().enumerate() {
                    if kept(declaration) {
                        continue;
                    }
                    let name = generator
                        .find(|name| !used.contains(name) && !taken(name))
                        .expect("names never run out");
                    used.push(name.clone());
                    names[start + index] = Some(name);
                }
            }
            start = end;
        }
        names
    }

    /// The names visible at an occurrence in `scope` from declarations below
    /// the top level, given the new `names` of [`Resolution::local_names`]: a
    /// top-level or free name read there must not be renamed to one of them.
    pub fn visible_locals<'n>(
        &'n self,
        mut scope: usize,
        names: &'n [Option<String>],
    ) -> Vec<&'n str> {
        let mut visible = Vec::new();
        while scope != 0 && scope != NO_SCOPE {
            for (index, declaration) in self.declarations.iter().enumerate() {
                if declaration.scope == scope {
                    visible.push(names[index].as_deref().unwrap_or(declaration.name));
                }
            }
            scope = self.parents[scope];
        }
        visible
    }
}

// ---------------------------------------------------------------------------
// Short names

/// Generated names in the order of Closure's `DefaultNameGenerator`
/// (`DefaultNameGenerator.java:59-70` in the pinned clone): one character
/// from `a-z`, `A-Z` and `$`, then two with `_` and digits allowed after the
/// first, and so on. Reserved words (and `let`, `yield`, `await`, `eval`,
/// `arguments`) and the ECMAScript globals are skipped.
#[derive(Default)]
pub struct NameGenerator {
    index: usize,
}

const FIRST_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ$";
const OTHER_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789$";

impl NameGenerator {
    /// The name at `index` of the unfiltered sequence.
    fn spell(mut index: usize) -> String {
        let mut count = FIRST_CHARS.len();
        let mut length = 1;
        while index >= count {
            index -= count;
            count *= OTHER_CHARS.len();
            length += 1;
        }
        let mut tail = Vec::with_capacity(length);
        for _ in 1..length {
            tail.push(OTHER_CHARS[index % OTHER_CHARS.len()]);
            index /= OTHER_CHARS.len();
        }
        tail.push(FIRST_CHARS[index]);
        tail.reverse();
        String::from_utf8(tail).expect("ASCII")
    }
}

impl Iterator for NameGenerator {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        loop {
            let name = Self::spell(self.index);
            self.index += 1;
            if !unavailable(&name) {
                return Some(name);
            }
        }
    }
}

/// Words a generated name must never be: reserved words, names with special
/// meaning in some context, and the ECMAScript globals.
pub fn unavailable(name: &str) -> bool {
    reserved(name)
        || matches!(
            name,
            "let" | "yield" | "await" | "eval" | "arguments" | "async" | "of" | "get" | "set"
        )
        || ECMASCRIPT_GLOBALS.binary_search(&name).is_ok()
}

// ---------------------------------------------------------------------------
// Terse writing

/// What [`TerseWriter`] needs to know of a token to separate it from its
/// neighbours: its kind and the text that decides (an identifier's original
/// spelling where a placeholder or a new name is written).
#[derive(Clone, Copy, Debug)]
pub struct Shape<'s> {
    pub kind: Kind,
    pub text: &'s str,
}

/// Joins tokens with the fewest separators that keep their meaning: a space
/// where two tokens would merge (`return a`, `a - -b`, `a / /re/`), and a line
/// break only where automatic semicolon insertion may depend on one, between
/// a token that may end a statement and one that may start the next (or after
/// `return`, `throw`, `break`, `continue` and `yield`). Every other line break,
/// all other whitespace and all comments are dropped, which never changes how
/// the tokens parse.
#[derive(Default)]
pub struct TerseWriter {
    pub out: String,
    previous: Option<Previous>,
}

#[derive(Clone, Copy)]
struct Previous {
    ends_statement: bool,
    restricted: bool,
    last: u8,
    plain_number: bool,
    regex: bool,
    /// The text of a punctuator of one or two characters, padded with a space.
    punct: [u8; 2],
}

fn word_byte(byte: u8) -> bool {
    identifier_part(byte) || byte == b'\\'
}

/// Whether a token of this shape may be the last of a statement that ASI
/// would end at a following line break.
fn may_end_statement(shape: Shape) -> bool {
    match shape.kind {
        Kind::Name => {
            !reserved(shape.text)
                || matches!(
                    shape.text,
                    "this" | "super" | "null" | "true" | "false" | "debugger"
                )
        }
        Kind::Punct => matches!(shape.text, ")" | "]" | "}" | "++" | "--"),
        Kind::TemplateHead | Kind::TemplateMiddle => false,
        _ => true,
    }
}

/// Whether a token of this shape may start a statement that a preceding
/// line break separates by ASI. Tokens that continue an expression (`(`,
/// `[`, `+`, `-`, `/`, `.`, operators) never trigger ASI, so their line
/// breaks can go.
fn may_start_statement(shape: Shape) -> bool {
    match shape.kind {
        Kind::Name => !matches!(
            shape.text,
            "in" | "instanceof" | "else" | "catch" | "finally"
        ),
        Kind::Punct => matches!(shape.text, "{" | "!" | "~" | "++" | "--"),
        Kind::TemplateMiddle | Kind::TemplateTail => false,
        _ => true,
    }
}

impl TerseWriter {
    /// Writes `text`, separated as a token of `shape` (`text` itself for most
    /// tokens) that follows a line break when `newline_before`.
    pub fn write(&mut self, shape: Shape, text: &str, newline_before: bool) {
        let first = shape.text.as_bytes()[0];
        if let Some(previous) = self.previous {
            let line = newline_before
                && ((previous.restricted && !matches!(shape.text, ";" | "}"))
                    || (previous.ends_statement && may_start_statement(shape)));
            let space = (word_byte(previous.last) && word_byte(first))
                || (previous.last == b'+' && first == b'+')
                || (previous.last == b'-' && first == b'-')
                || (previous.last == b'/' && matches!(first, b'/' | b'*'))
                || (previous.regex && word_byte(first))
                || (previous.plain_number && first == b'.')
                || (previous.last == b'<' && first == b'!')
                || (previous.punct == *b"--" && first == b'>');
            if line {
                self.out.push('\n');
            } else if space {
                self.out.push(' ');
            }
        }
        self.out.push_str(text);
        let decided = shape.text.as_bytes();
        let mut punct = [b' '; 2];
        if shape.kind == Kind::Punct && decided.len() <= 2 {
            punct[..decided.len()].copy_from_slice(decided);
        }
        self.previous = Some(Previous {
            ends_statement: may_end_statement(shape),
            restricted: shape.kind == Kind::Name
                && matches!(
                    shape.text,
                    "return" | "throw" | "break" | "continue" | "yield"
                ),
            last: *decided.last().unwrap(),
            plain_number: shape.kind == Kind::Number && decided.iter().all(u8::is_ascii_digit),
            regex: shape.kind == Kind::Regex,
            punct,
        });
    }

    /// Ends a sequence of statements that other text may follow on a new
    /// line: a line break if the last token may end a statement, so that a
    /// statement written after it starts as it did in the source.
    pub fn end_statements(&mut self) {
        if self
            .previous
            .is_some_and(|previous| previous.ends_statement)
        {
            self.out.push('\n');
        }
        self.previous = None;
    }

    /// Writes a token as it is.
    pub fn token(&mut self, source: &str, token: &Token) {
        let text = token.text(source);
        self.write(
            Shape {
                kind: token.kind,
                text,
            },
            text,
            token.newline_before,
        );
    }
}

/// For each bracket punctuator of `tokens`, the index of its partner; other
/// tokens and unmatched brackets have `usize::MAX`.
pub fn partners(source: &str, tokens: &[Token]) -> Vec<usize> {
    let mut partners = vec![usize::MAX; tokens.len()];
    let mut open: Vec<usize> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != Kind::Punct {
            continue;
        }
        let text = token.text(source);
        match text {
            "(" | "[" | "{" => open.push(index),
            ")" | "]" | "}" => {
                let expected = match text {
                    ")" => "(",
                    "]" => "[",
                    _ => "{",
                };
                if let Some(&opener) = open.last()
                    && tokens[opener].text(source) == expected
                {
                    open.pop();
                    partners[opener] = index;
                    partners[index] = opener;
                }
            }
            _ => {}
        }
    }
    partners
}

/// Whether the `)` at `close` ends the head of an `if`, `for`, `while` or
/// `with` statement, after which a statement starts.
pub fn closes_control_head(
    source: &str,
    tokens: &[Token],
    partners: &[usize],
    close: usize,
) -> bool {
    let open = partners[close];
    open != usize::MAX
        && open > 0
        && tokens[open - 1].kind == Kind::Name
        && matches!(
            tokens[open - 1].text(source),
            "if" | "for" | "while" | "with"
        )
}

/// The `;` tokens that can go: those right before a `}`, which ends the
/// statement anyway, unless the `;` is an empty statement that a statement
/// needs as its body (`if (a) ;`, `else ;`, `do ;`, `label: ;`, `case a: ;`).
pub fn redundant_semicolons(source: &str, tokens: &[Token], partners: &[usize]) -> Vec<bool> {
    let mut redundant = vec![false; tokens.len()];
    for index in 1..tokens.len().saturating_sub(1) {
        let token = &tokens[index];
        if token.kind != Kind::Punct
            || token.text(source) != ";"
            || tokens[index + 1].kind != Kind::Punct
            || tokens[index + 1].text(source) != "}"
        {
            continue;
        }
        let previous = &tokens[index - 1];
        let body = match (previous.kind, previous.text(source)) {
            (Kind::Name, "else" | "do") | (Kind::Punct, ":") => true,
            (Kind::Punct, ")") => closes_control_head(source, tokens, partners, index - 1),
            _ => false,
        };
        redundant[index] = !body;
    }
    redundant
}

/// `tokens` of `source`, tersely written ([`TerseWriter`]), without
/// [`redundant_semicolons`].
pub fn terse(source: &str, tokens: &[Token]) -> String {
    let partners = partners(source, tokens);
    let redundant = redundant_semicolons(source, tokens, &partners);
    let mut writer = TerseWriter::default();
    for (token, redundant) in tokens.iter().zip(redundant) {
        if !redundant {
            writer.token(source, token);
        }
    }
    writer.out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(body: &str) -> (Vec<&str>, Vec<&str>, bool) {
        let tokens =
            lex(body, 0..body.len()).unwrap_or_else(|e| panic!("{} at {}", e.message, e.offset));
        let facts = analyze(body, &tokens)
            .unwrap_or_else(|(offset, message)| panic!("{message} at {offset}"));
        (
            facts.declares,
            facts.references,
            facts.has_effect_statements,
        )
    }

    #[test]
    fn locals_shadow_runtime_bindings_through_every_scope_kind() {
        let (declares, references, effect) = facts(
            "function f(a, {b, c: [d = e], ...g} = h, ...i) {\n\
               const j = k; let [l, , m] = n; var o;\n\
               for (const [p, q] of r) s(p, q);\n\
               for (let t = 0; t < u; t++) {}\n\
               for (v in w) {}\n\
               try { x(); } catch ({message}) { y(message); }\n\
               if (z) { function inner() {} inner(); }\n\
               return a + b + c + d + g + i + j + l + m + o + arguments.length;\n\
             }",
        );
        assert_eq!(declares, ["f"]);
        // `c:` in the parameter pattern is a key, so the body's `c` is free.
        assert_eq!(
            references,
            [
                "c", "e", "h", "k", "n", "r", "s", "u", "v", "w", "x", "y", "z"
            ]
        );
        assert!(!effect);
    }

    #[test]
    fn properties_keys_and_labels_are_not_references() {
        let (_, references, _) = facts(
            "const o = {a: b, c, [d]: e, f() { return this.g; }, get h() { return i?.j; }, 'k': l, async m() {}, *n() {}, ...p};\n\
             outer: for (;;) { break outer; }\n\
             class Q extends R.S { static T = U; #v = 1; w(x) { return x.#v + super.y; } static { z; } }",
        );
        assert_eq!(
            references,
            ["R", "U", "b", "c", "d", "e", "i", "l", "p", "z"]
        );
    }

    #[test]
    fn arrows_regexes_and_templates_scope_and_lex_correctly() {
        let (declares, references, effect) = facts(
            "const re = /[/{]\\/(a)/g, div = a / b / c;\n\
             const f = (x, y = z) => x + y, g = w => ({w, v}), h = async q => q, k = async (r) => { return r; };\n\
             const t = `a${m + `b${n}`}c${ {o}.o }`;\n\
             if (re) /x/.test(p);\n\
             run(u => u, typeof s === 'string' ? 1 : 2, x => x ? y : z2);",
        );
        assert_eq!(declares, ["re", "div", "f", "g", "h", "k", "t"]);
        assert_eq!(
            references,
            [
                "a", "b", "c", "m", "n", "o", "p", "run", "s", "v", "y", "z", "z2"
            ]
        );
        assert!(effect);
    }

    #[test]
    fn statements_without_semicolons_still_split_at_declarations() {
        let (declares, references, _) =
            facts("const a = b\nconst c = a\nfunction d() { return e }\n");
        assert_eq!(declares, ["a", "c", "d"]);
        assert_eq!(references, ["b", "e"]);
    }

    #[test]
    fn malformed_bodies_are_rejected() {
        for body in [
            "function f() {",
            "const a = );",
            "const s = 'open",
            "const r = /x",
            "a ? b : c : d;",
        ] {
            let parsed = lex(body, 0..body.len())
                .map_err(|e| (e.offset, e.message.to_string()))
                .and_then(|tokens| analyze(body, &tokens).map(|_| ()));
            assert!(parsed.is_err(), "{body}");
        }
    }

    /// `body` with its locals renamed by [`Resolution::local_names`] and
    /// written by [`TerseWriter`], without dropping semicolons or joining
    /// declarations; top-level and free names keep their spelling.
    fn renamed(body: &str) -> String {
        let tokens = lex(body, 0..body.len()).unwrap();
        let resolution = resolve(body, &tokens).unwrap();
        let names = resolution.local_names(|_| false, unavailable);
        let mut writer = TerseWriter::default();
        for (index, token) in tokens.iter().enumerate() {
            let text = token.text(body);
            let occurrence = resolution.occurrences[index];
            let written = occurrence
                .and_then(|occurrence| occurrence.declaration)
                .and_then(|declaration| names[declaration].clone())
                .unwrap_or_else(|| text.to_owned());
            let shape = Shape {
                kind: token.kind,
                text,
            };
            if occurrence.is_some_and(|occurrence| occurrence.shorthand) && written != text {
                writer.write(shape, text, token.newline_before);
                let colon = Shape {
                    kind: Kind::Punct,
                    text: ":",
                };
                writer.write(colon, ":", false);
                writer.write(shape, &written, false);
            } else {
                writer.write(shape, &written, token.newline_before);
            }
        }
        writer.end_statements();
        writer.out.trim_end().to_owned()
    }

    #[test]
    fn locals_get_short_names_that_capture_nothing() {
        // Parameters in order; the top-level name and globals keep theirs.
        assert_eq!(
            renamed("function f(value, other) { return Math.max(value, other); }"),
            "function f(a,b){return Math.max(a,b);}"
        );
        // An inner scope starts again at `a`, skipping the outer names it reads.
        assert_eq!(
            renamed(
                "function f(x) { const g = function (y) { return x + y; }, h = (z) => z; return [g, h]; }"
            ),
            "function f(a){const b=function(b){return a+b;},c=(a)=>a;return[b,c];}"
        );
        // A shorthand property keeps its key; `arguments` keeps its name.
        assert_eq!(
            renamed("function f(cnt, {tail}) { return {cnt, tail, n: arguments.length}; }"),
            "function f(a,{tail:b}){return{cnt:a,tail:b,n:arguments.length};}"
        );
        // A catch parameter and its body's declarations share a scope.
        assert_eq!(
            renamed("try { g(); } catch (error) { const value = error; h(value); }"),
            "try{g();}catch(a){const b=a;h(b);}"
        );
        // A class or function expression's own name binds inside it.
        assert_eq!(
            renamed(
                "const make = () => { const Type = class Inner { m() { return Inner; } }; return Type; };"
            ),
            "const make=()=>{const a=class a{m(){return a;}};return a;};"
        );
    }

    #[test]
    fn top_level_names_avoid_the_locals_visible_where_they_are_read() {
        let body = "function f(x) { return (y) => g(x, y); }";
        let tokens = lex(body, 0..body.len()).unwrap();
        let resolution = resolve(body, &tokens).unwrap();
        let names = resolution.local_names(|_| false, unavailable);
        let g = tokens
            .iter()
            .position(|token| token.text(body) == "g")
            .unwrap();
        let occurrence = resolution.occurrences[g].unwrap();
        assert_eq!(occurrence.declaration, None);
        let mut visible = resolution.visible_locals(occurrence.scope, &names);
        visible.sort_unstable();
        assert_eq!(visible, ["a", "arguments", "b"]);
    }

    #[test]
    fn the_terse_writer_keeps_only_separators_the_parse_needs() {
        let written = |text: &str| terse(text, &lex_section(text, 0..text.len(), false).unwrap());
        // Line breaks matter only where ASI could insert a semicolon.
        assert_eq!(written("a\n(b)\n[c]\n+d"), "a(b)[c]+d");
        assert_eq!(written("a\nb\n++c"), "a\nb\n++c");
        assert_eq!(written("x = 1 // note\nreturn\nx"), "x=1\nreturn\nx");
        assert_eq!(written("{ return\n}"), "{return}");
        // Spaces keep tokens apart that would merge.
        assert_eq!(written("a - -b + +c"), "a- -b+ +c");
        assert_eq!(written("x = a / /re/g.exec(b) / c"), "x=a/ /re/g.exec(b)/c");
        assert_eq!(
            written("1 .toString(); 1.5.toFixed(); /a/ instanceof RegExp"),
            "1 .toString();1.5.toFixed();/a/ instanceof RegExp"
        );
        assert_eq!(
            written("a < !--b; typeof x === \"y\""),
            "a< !--b;typeof x===\"y\""
        );
        // A `;` before `}` goes unless a statement needs it as its body.
        assert_eq!(
            written("function f() { a(); if (b) ; while (c) ; }"),
            "function f(){a();if(b);while(c);}"
        );
        assert_eq!(written("{ x: ; }"), "{x:;}");
    }

    #[test]
    fn adjacent_declarations_join() {
        let text = "const a = 1; const b = () => { let c; let d = 2; return c; }; let e; var f; var g; const h = 3;";
        let tokens = lex(text, 0..text.len()).unwrap();
        let resolution = resolve(text, &tokens).unwrap();
        let joins = joined_declarations(text, &tokens, &resolution.statements);
        let mut writer = TerseWriter::default();
        for (token, join) in tokens.iter().zip(joins) {
            match join {
                Join::Keep => writer.token(text, token),
                Join::Comma => writer.write(
                    Shape {
                        kind: Kind::Punct,
                        text: ",",
                    },
                    ",",
                    false,
                ),
                Join::Drop => {}
            }
        }
        assert_eq!(
            writer.out,
            "const a=1,b=()=>{let c,d=2;return c;};let e;var f;var g;const h=3;"
        );
    }

    #[test]
    fn generated_names_follow_closure_order_and_skip_reserved_words() {
        let names = NameGenerator::default().take(600).collect::<Vec<_>>();
        assert_eq!(&names[..3], ["a", "b", "c"]);
        assert_eq!(names[26], "A");
        assert_eq!(names[52], "$");
        assert_eq!(&names[53..56], ["aa", "ab", "ac"]);
        for word in ["do", "if", "in", "of", "NaN", "Map", "Set", "let"] {
            assert!(!names.contains(&word.to_owned()), "{word}");
        }
    }
}
