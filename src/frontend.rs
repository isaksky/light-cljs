//! JavaScript source frontend: lexer and parser for the documented subset
//! (`docs/design.md` §§7, 12–14).
//!
//! The accepted grammar, the rejected forms and the known gaps are listed in
//! `docs/grammar.md`; that document and this parser are kept in agreement by
//! the tests in `frontend/tests.rs`. Lowering the AST to VM bytecode is a
//! later stage (pellet clj-quickjs-22); nothing here executes code.
//!
//! * [`parse_script`]: source text to an [`ast::Script`], or the first
//!   [`SyntaxError`]. No AST is produced for input that uses an unsupported
//!   form; the error names the form and its position.
//! * [`lexer::Lexer`]: on-demand tokenizer used by the parser. It never
//!   decides between division and a regular expression literal; the parser
//!   does, from grammatical context, and rejects regular expressions.
//!
//! Every AST node carries a byte-offset [`Span`] into the source.
//! [`line_column`] converts an offset into the 1-based line and column used by
//! diagnostics. Neither stage panics on any input: malformed input, unknown
//! characters, truncated input and nesting deeper than [`MAX_NESTING`] are all
//! reported as [`SyntaxError`]s.

pub mod ast;
pub mod lexer;
mod parser;

#[cfg(test)]
mod tests;

use std::fmt;

/// Nesting budget, in units: each nesting construct (parentheses, blocks,
/// functions, prefix operators, statement bodies, links of operator, member
/// and call chains) charges a cost weighted by the Rust stack it uses
/// (`docs/grammar.md`, "Nesting limit"). Input over budget is rejected with
/// [`SyntaxErrorKind::TooDeep`], so parsing at the limit fits a 2 MiB thread
/// stack even unoptimized, and the AST depth stays bounded for dropping it
/// and for later recursive passes.
pub const MAX_NESTING: usize = 256;

/// Half-open byte range `start..end` into the source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Span {
        Span { start, end }
    }

    /// The smallest span covering both `self` and `other`.
    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }

    /// The source text this span covers.
    pub fn text(self, source: &str) -> &str {
        source.get(self.start..self.end).unwrap_or("")
    }
}

/// Category of a [`SyntaxError`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxErrorKind {
    /// A character or character sequence that is not a token of the subset
    /// (for example `@`, `#`, or a non-ASCII identifier character).
    UnknownToken,
    /// A malformed token: an unterminated string or comment, a bad escape, or
    /// a malformed number literal.
    InvalidToken,
    /// The input ended inside a construct (truncated input).
    UnexpectedEnd,
    /// A token the grammar does not allow at this position.
    UnexpectedToken,
    /// Valid JavaScript that this subset deliberately does not support
    /// (classes, `async`, regular expression literals, `for`, ...).
    Unsupported,
    /// A reserved word (or a name the subset reserves) used as a binding
    /// name, assignment target or reference.
    ReservedWord,
    /// Syntax that JavaScript itself rejects as an early error (for example
    /// `return` outside a function, a `const` without an initializer, or a
    /// duplicate parameter name).
    Invalid,
    /// Nesting deeper than [`MAX_NESTING`].
    TooDeep,
}

/// A positioned frontend diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    pub kind: SyntaxErrorKind,
    pub message: String,
    /// The offending source range.
    pub span: Span,
    /// 1-based line of `span.start`.
    pub line: u32,
    /// 1-based column of `span.start`, counted in Unicode scalar values.
    pub column: u32,
}

impl SyntaxError {
    pub(crate) fn new(
        source: &str,
        kind: SyntaxErrorKind,
        span: Span,
        message: impl Into<String>,
    ) -> SyntaxError {
        let (line, column) = line_column(source, span.start);
        SyntaxError {
            kind,
            message: message.into(),
            span,
            line,
            column,
        }
    }
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: SyntaxError: {}",
            self.line, self.column, self.message
        )
    }
}

impl std::error::Error for SyntaxError {}

/// 1-based `(line, column)` of byte `offset` in `source`. Lines end at LF,
/// CR, CRLF (one terminator), U+2028 and U+2029; columns count Unicode scalar
/// values. An offset past the end (or inside a multi-byte character) is
/// clamped to the preceding character boundary.
pub fn line_column(source: &str, offset: usize) -> (u32, u32) {
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let mut line: u32 = 1;
    let mut column: u32 = 1;
    let mut chars = source[..offset].chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                line = line.saturating_add(1);
                column = 1;
            }
            '\n' | '\u{2028}' | '\u{2029}' => {
                line = line.saturating_add(1);
                column = 1;
            }
            _ => column = column.saturating_add(1),
        }
    }
    (line, column)
}

/// Parse `source` as a script of the documented subset (`docs/grammar.md`).
///
/// All code is treated as strict-mode code. Returns the first error; no AST is
/// produced for input containing an unsupported or malformed form.
pub fn parse_script(source: &str) -> Result<ast::Script, SyntaxError> {
    parser::Parser::new(source).parse_script()
}
