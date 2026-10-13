//! Borrowing syntax parsing, lexical analysis, and JavaScript emission for a
//! deliberately small executable ClojureScript core.
//!
//! Tokens retain raw byte spans, including quotes, escapes, and reader prefixes.
//! Whitespace and commas are skipped; comments are emitted. Atom classification,
//! escape decoding/validation, delimiter matching, and reader-macro semantics
//! belong to the parser or subsequent reader expansion. See docs/lexer.md for the
//! token contract and docs/parser.md for the syntax/semantics boundary.

use std::{iter::FusedIterator, ops::Range};

pub mod analyzer;
pub mod application;
mod char_classes;
mod compilation_session;
pub mod compiler;
mod compiler_binding_macros;
pub mod compiler_data;
mod compiler_destructuring;
mod compiler_libraries;
pub mod compiler_names;
pub mod compiler_reader;
pub mod emitter;
pub mod eval_session;
pub mod evaluator;
mod program_fragment;
pub use eval_session::{
    EvalDiagnostic, EvalOptions, EvalResult, EvalSession, HostArity, HostCall, HostFunction,
};
mod eval_compiler;
pub mod form_bridge;
mod literals;
pub mod macro_host;
pub mod parser;
mod reader_literals;
pub mod source_provider;

pub use analyzer::{
    AnalysisError, AnalysisErrorKind, CoreFnId, Expr, ExprContext, ExprId, ExprKind, GlobalBinding,
    GlobalId, JsGlobal, JsGlobalId, LocalBinding, LocalId, NamedLiteral, NamedLiteralId,
    NamespaceInput, Program, Span, StringId, analyze, analyze_namespaces,
    analyze_namespaces_with_macros, analyze_with_macros,
};
pub use application::{
    ApplicationOptions, ApplicationStats, PreparedApplication, prepare_application,
};
pub use compiler::{
    CompileError, NamespaceCompileError, compile, compile_namespace, compile_namespace_with_macros,
    compile_with_macros,
};
pub use emitter::emit_js;
pub use macro_host::{MacroHost, MacroImport, MacroLimits};

pub use parser::{
    CollectionKind, ErrorKind, LineIndex, Node, NodeId, NodeKind, NumberKind, ParseError, Parser,
    PrefixKind, StringValue, SymbolicValue, SyntaxTree, parse,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LexError {
    UnterminatedString,
    UnterminatedRegex,
    MissingCharacter,
    MissingDispatch,
    UnreadableForm,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenKind {
    OpenParen,
    CloseParen,
    OpenBracket,
    CloseBracket,
    OpenBrace,
    CloseBrace,
    Atom,
    String,
    Regex,
    Character,
    Comment,
    Quote,
    SyntaxQuote,
    Unquote,
    UnquoteSplicing,
    Deref,
    Metadata,
    SetStart,
    AnonymousFnStart,
    VarQuote,
    Discard,
    ReaderConditional,
    ReaderConditionalSplicing,
    NamespacedMap,
    AutoNamespacedMap,
    SymbolicValue,
    ReadEval,
    /// A bare `#` prefix, followed by a separate tag atom (e.g. `#js`).
    Dispatch,
    Error(LexError),
}

/// A kind and a half-open byte span into the original source; no text is owned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Range<usize>,
}

/// A byte scanner borrowing UTF-8 source, with no reader or nesting state.
#[derive(Clone, Debug)]
pub struct Tokenizer<'a> {
    source: &'a str,
    cursor: usize,
}

const SPACE: u8 = 1;
const TERMINATING: u8 = 2;
const MACRO: u8 = 4;

// tools.reader distinguishes macro characters from macro-terminating characters.
// In particular, #, ', :, and % can occur inside a symbol or keyword, but end a
// number candidate. Classification still belongs to the reader.
const CLASSES: [u8; 256] = {
    let mut table = [0; 256];
    let mut byte = b'\t';
    while byte <= b'\r' {
        table[byte as usize] = SPACE;
        byte += 1;
    }
    byte = 0x1c;
    while byte <= b' ' {
        table[byte as usize] = SPACE;
        byte += 1;
    }
    table[b',' as usize] = SPACE;
    let terminating = b"\";@^`~()[]{}\\";
    let mut i = 0;
    while i < terminating.len() {
        table[terminating[i] as usize] = TERMINATING | MACRO;
        i += 1;
    }
    table[b'#' as usize] = MACRO;
    table[b'\'' as usize] = MACRO;
    table[b':' as usize] = MACRO;
    table[b'%' as usize] = MACRO;
    table
};

// The non-ASCII Character.isWhitespace set used by the JVM tools.reader.
// All members occupy three UTF-8 bytes. NBSP, NEL, figure space, narrow NBSP,
// zero-width space, and BOM are deliberately absent. No Unicode decoding is
// needed in the byte loops; UTF-8 continuation bytes cannot match these leads.
#[inline]
fn unicode_space(bytes: &[u8], cursor: usize) -> bool {
    matches!(bytes.get(cursor..),
        Some([0xe1, 0x9a, 0x80, ..])
        | Some([0xe2, 0x80, 0x80..=0x86 | 0x88..=0x8a | 0xa8 | 0xa9, ..])
        | Some([0xe2, 0x81, 0x9f, ..])
        | Some([0xe3, 0x80, 0x80, ..]))
}

impl<'a> Tokenizer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self { source, cursor: 0 }
    }

    fn digit_at(&self, cursor: usize) -> bool {
        match self.source.as_bytes().get(cursor) {
            Some(byte) if byte.is_ascii_digit() => true,
            Some(byte) if *byte >= 0x80 => char_classes::unicode_digit(
                self.source[cursor..]
                    .chars()
                    .next()
                    .expect("nonempty character"),
            ),
            _ => false,
        }
    }

    #[inline]
    fn eat(&mut self, byte: u8) -> bool {
        if self.source.as_bytes().get(self.cursor) == Some(&byte) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    #[inline]
    fn atom(&mut self, mask: u8) {
        let bytes = self.source.as_bytes();
        while self.cursor < bytes.len() {
            let byte = bytes[self.cursor];
            if CLASSES[byte as usize] & mask != 0
                || (byte >= 0x80 && unicode_space(bytes, self.cursor))
            {
                break;
            }
            self.cursor += 1;
        }
    }

    fn comment(&mut self) -> TokenKind {
        let bytes = self.source.as_bytes();
        while self.cursor < bytes.len() && !matches!(bytes[self.cursor], b'\r' | b'\n') {
            self.cursor += 1;
        }
        TokenKind::Comment
    }

    // Strings and regexes share quote/backslash boundary rules. Escape spelling
    // and regex validity are intentionally left to the reader. A backslash
    // protects the next byte from delimiter recognition; remaining UTF-8 bytes
    // are ordinary content, so returning at quote/EOF still preserves boundaries.
    fn quoted(&mut self, regex: bool) -> TokenKind {
        let bytes = self.source.as_bytes();
        while self.cursor < bytes.len() {
            let byte = bytes[self.cursor];
            self.cursor += 1;
            match byte {
                b'"' => {
                    return if regex {
                        TokenKind::Regex
                    } else {
                        TokenKind::String
                    };
                }
                b'\\' if self.cursor < bytes.len() => self.cursor += 1,
                _ => {}
            }
        }
        TokenKind::Error(if regex {
            LexError::UnterminatedRegex
        } else {
            LexError::UnterminatedString
        })
    }

    fn character(&mut self) -> TokenKind {
        let Some(ch) = self.source[self.cursor..].chars().next() else {
            return TokenKind::Error(LexError::MissingCharacter);
        };
        // The first character is always literal, even a delimiter or whitespace.
        // A CRLF/CRFF pair is one logical character in the compiler's indexing reader.
        self.cursor += ch.len_utf8();
        if ch == '\r' {
            if !self.eat(b'\n') {
                self.eat(b'\x0c');
            }
        } else if !(ch.is_ascii() && CLASSES[ch as usize] & (SPACE | TERMINATING) != 0)
            && !unicode_space(self.source.as_bytes(), self.cursor - ch.len_utf8())
        {
            self.atom(SPACE | TERMINATING);
        }
        TokenKind::Character
    }

    fn dispatch(&mut self) -> TokenKind {
        use TokenKind::*;
        let Some(&byte) = self.source.as_bytes().get(self.cursor) else {
            return Error(LexError::MissingDispatch);
        };
        self.cursor += 1;
        match byte {
            b'{' => SetStart,
            b'(' => AnonymousFnStart,
            b'"' => self.quoted(true),
            b'!' => self.comment(),
            b'\'' => VarQuote,
            b'_' => Discard,
            b'^' => Metadata, // Legacy metadata syntax accepted by tools.reader.
            b'=' => ReadEval,
            b'?' => {
                if self.eat(b'@') {
                    ReaderConditionalSplicing
                } else {
                    ReaderConditional
                }
            }
            b':' => {
                let kind = if self.eat(b':') {
                    AutoNamespacedMap
                } else {
                    NamespacedMap
                };
                // A namespace header uses symbol-style boundaries, even when
                // it starts with a digit. Keep it attached to its prefix.
                self.atom(SPACE | TERMINATING);
                kind
            }
            b'#' => SymbolicValue,
            b'<' => Error(LexError::UnreadableForm),
            _ => {
                // Tags are ordinary reader forms; don't swallow their first byte,
                // which might also be the lead byte of a multibyte character.
                self.cursor -= 1;
                Dispatch
            }
        }
    }
}

impl Iterator for Tokenizer<'_> {
    type Item = Token;

    // Retained after two release runs improved real-source throughput; see benchmarks/README.md.
    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self.source.as_bytes();
        while self.cursor < bytes.len() {
            let byte = bytes[self.cursor];
            if CLASSES[byte as usize] & SPACE != 0 {
                self.cursor += 1;
            } else if byte >= 0x80 && unicode_space(bytes, self.cursor) {
                self.cursor += 3;
            } else {
                break;
            }
        }

        let start = self.cursor;
        let first = *bytes.get(start)?;
        self.cursor += 1;
        use TokenKind::*;
        let kind = match first {
            b'(' => OpenParen,
            b')' => CloseParen,
            b'[' => OpenBracket,
            b']' => CloseBracket,
            b'{' => OpenBrace,
            b'}' => CloseBrace,
            b'"' => self.quoted(false),
            b'\\' => self.character(),
            b';' => self.comment(),
            b'\'' => Quote,
            b'`' => SyntaxQuote,
            b'~' => {
                if self.eat(b'@') {
                    UnquoteSplicing
                } else {
                    Unquote
                }
            }
            b'@' => Deref,
            b'^' => Metadata,
            b'#' => self.dispatch(),
            _ => {
                let numeric = first.is_ascii_digit()
                    || (first >= 0x80 && self.digit_at(start))
                    || (matches!(first, b'+' | b'-') && self.digit_at(self.cursor));
                self.atom(SPACE | if numeric { MACRO } else { TERMINATING });
                Atom
            }
        };
        Some(Token {
            kind,
            span: start..self.cursor,
        })
    }
}

impl FusedIterator for Tokenizer<'_> {}

mod compiler_bootstrap;
