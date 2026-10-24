//! On-demand tokenizer for the documented JavaScript subset
//! (`docs/grammar.md` §Lexical grammar).
//!
//! The lexer recognizes every ECMAScript punctuator by maximal munch, so that
//! an operator outside the subset (`**`, `>>>=`, `?.`, `=>`, ...) arrives at
//! the parser as one token and is rejected there with a precise diagnostic.
//! A `/` is always lexed as the division punctuator: the parser decides from
//! context whether a regular expression literal would have started, and
//! rejects it. Template literals, BigInt literals and numeric separators are
//! rejected here.
//!
//! Identifiers are Unicode: a start character is `ID_Start`, `$` or `_`, a
//! later one `ID_Continue`, `$`, ZWNJ or ZWJ (ECMAScript §12.7; tables in
//! `unicode_ident.rs`), and either may be written as a `\u` escape. A name is
//! its code points after escapes are decoded, compared without
//! normalization: `a\u0062c` is `abc`, while `Ãµ` (U+00C3 U+00B5) and `õ`
//! (U+00F5) are different names.

use std::cmp::Ordering;

use super::unicode_ident::{ID_CONTINUE, ID_START};
use super::{Span, SyntaxError, SyntaxErrorKind};
use crate::value::JsString;

/// A lexical token.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// A line terminator (possibly inside a comment) separates this token
    /// from the previous one. Drives automatic semicolon insertion.
    pub newline_before: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Identifier(String),
    Keyword(Keyword),
    /// A numeric literal's value.
    Number(f64),
    /// A string literal's cooked value: UTF-16 code units, possibly with
    /// lone surrogates (clj-quickjs-141).
    String(JsString),
    Punct(Punct),
    Eof,
}

impl TokenKind {
    /// Short human-readable description for diagnostics.
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Identifier(name) => format!("identifier `{name}`"),
            TokenKind::Keyword(k) => format!("keyword `{}`", k.as_str()),
            TokenKind::Number(_) => "number literal".to_string(),
            TokenKind::String(_) => "string literal".to_string(),
            TokenKind::Punct(p) => format!("`{}`", p.as_str()),
            TokenKind::Eof => "end of input".to_string(),
        }
    }
}

macro_rules! keywords {
    ($($variant:ident = $text:literal,)*) => {
        /// ECMAScript reserved words, including the strict-mode reserved
        /// words (`let`, `static`, `implements`, ...) and `await`, which this
        /// subset always reserves. All are rejected as identifiers.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Keyword { $($variant,)* }

        impl Keyword {
            pub fn as_str(self) -> &'static str {
                match self { $(Keyword::$variant => $text,)* }
            }

            pub fn from_text(text: &str) -> Option<Keyword> {
                match text { $($text => Some(Keyword::$variant),)* _ => None }
            }
        }
    };
}

keywords! {
    Await = "await",
    Break = "break",
    Case = "case",
    Catch = "catch",
    Class = "class",
    Const = "const",
    Continue = "continue",
    Debugger = "debugger",
    Default = "default",
    Delete = "delete",
    Do = "do",
    Else = "else",
    Enum = "enum",
    Export = "export",
    Extends = "extends",
    False = "false",
    Finally = "finally",
    For = "for",
    Function = "function",
    If = "if",
    Implements = "implements",
    Import = "import",
    In = "in",
    Instanceof = "instanceof",
    Interface = "interface",
    Let = "let",
    New = "new",
    Null = "null",
    Package = "package",
    Private = "private",
    Protected = "protected",
    Public = "public",
    Return = "return",
    Static = "static",
    Super = "super",
    Switch = "switch",
    This = "this",
    Throw = "throw",
    True = "true",
    Try = "try",
    Typeof = "typeof",
    Var = "var",
    Void = "void",
    While = "while",
    With = "with",
    Yield = "yield",
}

macro_rules! puncts {
    ($($variant:ident = $text:literal,)*) => {
        /// Every ECMAScript punctuator. Most are outside the subset; they are
        /// lexed so that the parser can name them in its diagnostics.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Punct { $($variant,)* }

        impl Punct {
            pub fn as_str(self) -> &'static str {
                match self { $(Punct::$variant => $text,)* }
            }
        }

        /// Longest first, for maximal munch.
        const PUNCTS: &[(&str, Punct)] = &[$(($text, Punct::$variant),)*];
    };
}

puncts! {
    UShrAssign = ">>>=",
    Ellipsis = "...",
    StrictEq = "===",
    StrictNotEq = "!==",
    StarStarAssign = "**=",
    ShlAssign = "<<=",
    ShrAssign = ">>=",
    UShr = ">>>",
    AmpAmpAssign = "&&=",
    PipePipeAssign = "||=",
    NullishAssign = "??=",
    Arrow = "=>",
    Eq = "==",
    NotEq = "!=",
    Le = "<=",
    Ge = ">=",
    AmpAmp = "&&",
    PipePipe = "||",
    Nullish = "??",
    OptionalChain = "?.",
    PlusPlus = "++",
    MinusMinus = "--",
    StarStar = "**",
    Shl = "<<",
    Shr = ">>",
    PlusAssign = "+=",
    MinusAssign = "-=",
    StarAssign = "*=",
    SlashAssign = "/=",
    PercentAssign = "%=",
    AmpAssign = "&=",
    PipeAssign = "|=",
    CaretAssign = "^=",
    LBrace = "{",
    RBrace = "}",
    LParen = "(",
    RParen = ")",
    LBracket = "[",
    RBracket = "]",
    Semicolon = ";",
    Comma = ",",
    Dot = ".",
    Question = "?",
    Colon = ":",
    Assign = "=",
    Lt = "<",
    Gt = ">",
    Plus = "+",
    Minus = "-",
    Star = "*",
    Slash = "/",
    Percent = "%",
    Bang = "!",
    Tilde = "~",
    Amp = "&",
    Pipe = "|",
    Caret = "^",
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// WhiteSpace (ECMAScript §12.2): TAB, VT, FF, ZWNBSP and Unicode `Zs`.
fn is_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\u{0B}' | '\u{0C}' | ' ' | '\u{A0}' | '\u{FEFF}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

/// Whether `c` lies in one of `table`'s sorted, disjoint, inclusive ranges.
fn in_ranges(table: &[(u32, u32)], c: char) -> bool {
    let cp = u32::from(c);
    table
        .binary_search_by(|&(lo, hi)| {
            if hi < cp {
                Ordering::Less
            } else if lo > cp {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        })
        .is_ok()
}

/// IdentifierStartChar: `ID_Start`, `$` or `_`.
fn is_ident_start(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphabetic() || c == '$' || c == '_'
    } else {
        in_ranges(ID_START, c)
    }
}

/// IdentifierPartChar: `ID_Continue`, `$`, ZWNJ or ZWJ.
fn is_ident_part(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphanumeric() || c == '$' || c == '_'
    } else {
        matches!(c, '\u{200C}' | '\u{200D}') || in_ranges(ID_CONTINUE, c)
    }
}

/// The tokenizer. Cloning it is cheap and gives an independent cursor, which
/// the parser uses for bounded lookahead.
#[derive(Clone, Debug)]
pub struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    /// The next token is in an IdentifierName position (a property name:
    /// after `.`, or where the parser asked for one with
    /// [`Lexer::expect_property_name`]), where a reserved word written with
    /// escapes is an ordinary name.
    after_dot: bool,
    /// The code being lexed is strict-mode code, where legacy octal and
    /// leading-zero number literals are early errors (clj-quickjs-51). The
    /// parser keeps it in step with its own mode ([`Lexer::set_strict`]).
    strict: bool,
}

impl<'a> Lexer<'a> {
    /// A lexer at the start of `src`, past a hashbang comment (ECMAScript
    /// HashbangComment, QuickJS `skip_shebang`): `#!` at byte 0 up to, not
    /// including, the first line terminator, which still counts for ASI and
    /// line numbers. `#` anywhere else is not a token.
    pub fn new(src: &'a str) -> Lexer<'a> {
        let pos = if src.starts_with("#!") {
            src.find(is_line_terminator).unwrap_or(src.len())
        } else {
            0
        };
        Lexer {
            src,
            pos,
            after_dot: false,
            strict: true,
        }
    }

    /// The next token is an object literal's property name (an
    /// IdentifierName position, clj-quickjs-114): a reserved word written
    /// with escapes lexes as an identifier there. One-shot: it applies to
    /// the next token only.
    pub(crate) fn expect_property_name(&mut self) {
        self.after_dot = true;
    }

    /// Lex what follows as strict-mode code if `strict`, else as sloppy
    /// code (clj-quickjs-51). A new lexer is strict.
    pub(crate) fn set_strict(&mut self, strict: bool) {
        self.strict = strict;
    }

    pub fn source(&self) -> &'a str {
        self.src
    }

    fn peek_char(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek_char_at(&self, n: usize) -> Option<char> {
        self.src[self.pos..].chars().nth(n)
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek_char()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn error(&self, kind: SyntaxErrorKind, span: Span, message: impl Into<String>) -> SyntaxError {
        SyntaxError::new(self.src, kind, span, message)
    }

    /// Error spanning the single character at `pos` (or empty at the end).
    fn error_at(
        &self,
        pos: usize,
        kind: SyntaxErrorKind,
        message: impl Into<String>,
    ) -> SyntaxError {
        let end = self.src[pos..]
            .chars()
            .next()
            .map_or(pos, |c| pos + c.len_utf8());
        self.error(kind, Span::new(pos, end), message)
    }

    /// Skip whitespace and comments; report whether a line terminator was
    /// crossed.
    fn skip_trivia(&mut self) -> Result<bool, SyntaxError> {
        let mut newline = false;
        loop {
            let Some(c) = self.peek_char() else {
                return Ok(newline);
            };
            if is_whitespace(c) {
                self.bump();
            } else if is_line_terminator(c) {
                newline = true;
                self.bump();
            } else if self.rest().starts_with("//") {
                while let Some(c) = self.peek_char() {
                    if is_line_terminator(c) {
                        break;
                    }
                    self.bump();
                }
            } else if self.rest().starts_with("/*") {
                let start = self.pos;
                self.pos += 2;
                match self.rest().find("*/") {
                    Some(len) => {
                        let body = &self.rest()[..len];
                        if body.chars().any(is_line_terminator) {
                            newline = true;
                        }
                        self.pos += len + 2;
                    }
                    None => {
                        return Err(self.error(
                            SyntaxErrorKind::UnexpectedEnd,
                            Span::new(start, start + 2),
                            "unterminated block comment",
                        ));
                    }
                }
            } else {
                return Ok(newline);
            }
        }
    }

    /// Lex the next token. At the end of input this returns an `Eof` token
    /// (repeatedly).
    pub fn next_token(&mut self) -> Result<Token, SyntaxError> {
        let newline_before = self.skip_trivia()?;
        let start = self.pos;
        let kind = match self.peek_char() {
            None => TokenKind::Eof,
            Some(c) if is_ident_start(c) || c == '\\' => self.lex_identifier()?,
            Some(c) if c.is_ascii_digit() => self.lex_number()?,
            Some('.') if self.peek_char_at(1).is_some_and(|c| c.is_ascii_digit()) => {
                self.lex_number()?
            }
            Some(q @ ('"' | '\'')) => self.lex_string(q)?,
            Some('`') => {
                return Err(self.error_at(
                    start,
                    SyntaxErrorKind::Unsupported,
                    "template literals are not supported",
                ));
            }
            Some(c) => match self.lex_punct() {
                Some(p) => TokenKind::Punct(p),
                None => {
                    return Err(self.error_at(
                        start,
                        SyntaxErrorKind::UnknownToken,
                        format!("unexpected character {c:?}"),
                    ));
                }
            },
        };
        self.after_dot = kind == TokenKind::Punct(Punct::Dot);
        Ok(Token {
            kind,
            span: Span::new(start, self.pos),
            newline_before,
        })
    }

    /// IdentifierName: start character, then part characters, each possibly
    /// a `\u` escape. Without escapes, a reserved word is a keyword token.
    /// With escapes it is never the keyword (ECMAScript §12.7.2): after `.`
    /// or as an object literal key it is an ordinary property name, and anywhere else, where the subset
    /// always reserves it, it is rejected here.
    fn lex_identifier(&mut self) -> Result<TokenKind, SyntaxError> {
        let start = self.pos;
        let mut name = String::new();
        let mut escaped = false;
        loop {
            let first = self.pos == start;
            let c = match self.peek_char() {
                Some('\\') => {
                    escaped = true;
                    self.identifier_escape(first)?
                }
                Some(c)
                    if (if first {
                        is_ident_start(c)
                    } else {
                        is_ident_part(c)
                    }) =>
                {
                    self.bump();
                    c
                }
                _ => break,
            };
            name.push(c);
        }
        Ok(match Keyword::from_text(&name) {
            None => TokenKind::Identifier(name),
            Some(k) if !escaped => TokenKind::Keyword(k),
            Some(_) if self.after_dot => TokenKind::Identifier(name),
            Some(k) => {
                return Err(self.error(
                    SyntaxErrorKind::ReservedWord,
                    Span::new(start, self.pos),
                    format!(
                        "`{}` is the reserved word `{}` written with escapes, which cannot be used as an identifier",
                        &self.src[start..self.pos],
                        k.as_str()
                    ),
                ));
            }
        })
    }

    /// A `\uHHHH` or `\u{H...}` escape in an identifier, at the backslash.
    /// The code point must itself be a start character (`first`) or a part
    /// character.
    fn identifier_escape(&mut self, first: bool) -> Result<char, SyntaxError> {
        let backslash = self.pos;
        self.bump();
        let value = if self.peek_char() == Some('u') {
            self.bump();
            self.unicode_escape_value()
        } else {
            None
        };
        let Some(value) = value else {
            return Err(self.error_at(
                backslash,
                SyntaxErrorKind::InvalidToken,
                "invalid escape in identifier: expected `\\uHHHH` or `\\u{H...}`",
            ));
        };
        let span = Span::new(backslash, self.pos);
        let valid = if first { is_ident_start } else { is_ident_part };
        match char::from_u32(value).filter(|&c| valid(c)) {
            Some(c) => Ok(c),
            None => Err(self.error(
                SyntaxErrorKind::InvalidToken,
                span,
                format!(
                    "escaped code point U+{value:04X} cannot {} an identifier",
                    if first { "start" } else { "appear in" }
                ),
            )),
        }
    }

    fn lex_punct(&mut self) -> Option<Punct> {
        let rest = self.rest();
        for &(text, punct) in PUNCTS {
            if rest.starts_with(text) {
                // `a?.5:b` is `a ? .5 : b`, not optional chaining.
                if punct == Punct::OptionalChain
                    && rest[2..].starts_with(|c: char| c.is_ascii_digit())
                {
                    continue;
                }
                self.pos += text.len();
                return Some(punct);
            }
        }
        None
    }

    fn eat_digits(&mut self, radix: u32) -> Result<usize, SyntaxError> {
        let mut count = 0;
        loop {
            match self.peek_char() {
                Some(c) if c.is_digit(radix) => {
                    self.bump();
                    count += 1;
                }
                Some('_') => {
                    return Err(self.error_at(
                        self.pos,
                        SyntaxErrorKind::Unsupported,
                        "numeric separators are not supported",
                    ));
                }
                _ => return Ok(count),
            }
        }
    }

    fn lex_number(&mut self) -> Result<TokenKind, SyntaxError> {
        let start = self.pos;
        let rest = self.rest();
        let radix = match rest.get(..2) {
            Some("0x" | "0X") => 16,
            Some("0o" | "0O") => 8,
            Some("0b" | "0B") => 2,
            _ => 10,
        };
        // `bigint_suffix`: a BigInt suffix `n` would be valid after the
        // literal (it is unsupported): an integer with no leading zero.
        let (value, bigint_suffix) = if radix != 10 {
            self.pos += 2;
            let digits_start = self.pos;
            if self.eat_digits(radix)? == 0 {
                return Err(self.error(
                    SyntaxErrorKind::InvalidToken,
                    Span::new(start, self.pos),
                    "missing digits after radix prefix",
                ));
            }
            (radix_value(&self.src[digits_start..self.pos], radix), true)
        } else if is_leading_zero_literal(rest) {
            if self.strict {
                return Err(leading_zero_in_strict_code(self.src, start));
            }
            (self.lex_leading_zero_literal(start)?, false)
        } else {
            self.eat_digits(10)?;
            self.lex_decimal_tail(start)?
        };
        match self.peek_char() {
            Some('n') if bigint_suffix => Err(self.error(
                SyntaxErrorKind::Unsupported,
                Span::new(start, self.pos + 1),
                "BigInt literals are not supported",
            )),
            Some(c) if is_ident_start(c) || c.is_ascii_digit() || c == '\\' => Err(self.error_at(
                self.pos,
                SyntaxErrorKind::InvalidToken,
                "identifier or digit starts immediately after number literal",
            )),
            _ => Ok(TokenKind::Number(value)),
        }
    }

    /// After the integer digits of a decimal literal starting at `start`:
    /// an optional fraction and exponent. Returns the literal's value and
    /// whether it is an integer (no fraction or exponent).
    fn lex_decimal_tail(&mut self, start: usize) -> Result<(f64, bool), SyntaxError> {
        let mut integral = true;
        if self.peek_char() == Some('.') {
            integral = false;
            self.bump();
            self.eat_digits(10)?;
        }
        if matches!(self.peek_char(), Some('e' | 'E')) {
            integral = false;
            self.bump();
            if matches!(self.peek_char(), Some('+' | '-')) {
                self.bump();
            }
            if self.eat_digits(10)? == 0 {
                return Err(self.error(
                    SyntaxErrorKind::InvalidToken,
                    Span::new(start, self.pos),
                    "missing exponent digits in number literal",
                ));
            }
        }
        // Rust's float parser is correctly rounded, overflows to infinity,
        // ignores leading zeros, and accepts every form lexed above ("1.",
        // ".5", "1.e3", "09.5"). A failure here would be a lexer bug;
        // report it.
        let value = self.src[start..self.pos].parse::<f64>().map_err(|_| {
            self.error(
                SyntaxErrorKind::InvalidToken,
                Span::new(start, self.pos),
                "malformed number literal",
            )
        })?;
        Ok((value, integral))
    }

    /// Sloppy code only: a literal starting with `0` and a digit
    /// (ECMAScript B.1.1; QuickJS `js_atof` with `ATOD_ACCEPT_LEGACY_OCTAL`).
    /// If all its digits are octal it is a LegacyOctalIntegerLiteral
    /// (`017` is 15), which ends there: a following `.` is a member access
    /// (`01.a`) and an exponent is an identifier right after the literal.
    /// Otherwise (an `8` or `9` among them) it is a
    /// NonOctalDecimalIntegerLiteral, a decimal literal that may take a
    /// fraction and exponent (`09.5`, `08e1`). Neither form allows a
    /// numeric separator in its integer digits.
    fn lex_leading_zero_literal(&mut self, start: usize) -> Result<f64, SyntaxError> {
        self.bump();
        let digits_start = self.pos;
        while self.peek_char().is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
        }
        if self.peek_char() == Some('_') {
            return Err(self.error_at(
                self.pos,
                SyntaxErrorKind::InvalidToken,
                "a numeric separator cannot appear in a number literal with a leading 0",
            ));
        }
        let digits = &self.src[digits_start..self.pos];
        if digits.bytes().all(|b| b < b'8') {
            Ok(radix_value(digits, 8))
        } else {
            Ok(self.lex_decimal_tail(start)?.0)
        }
    }

    fn lex_string(&mut self, quote: char) -> Result<TokenKind, SyntaxError> {
        let start = self.pos;
        self.bump();
        let mut out: Vec<u16> = Vec::new();
        loop {
            let char_pos = self.pos;
            let Some(c) = self.bump() else {
                return Err(self.error(
                    SyntaxErrorKind::UnexpectedEnd,
                    Span::new(start, self.pos),
                    "unterminated string literal",
                ));
            };
            match c {
                c if c == quote => break,
                '\n' | '\r' => {
                    return Err(self.error(
                        SyntaxErrorKind::InvalidToken,
                        Span::new(start, char_pos),
                        "unterminated string literal (line break before closing quote)",
                    ));
                }
                '\\' => self.lex_escape(char_pos, &mut out)?,
                c => push_char(&mut out, c),
            }
        }
        // Only a source text of over 2^30 - 1 code units can hold such a
        // literal; report it rather than truncate.
        JsString::from_code_units(out)
            .map(TokenKind::String)
            .ok_or_else(|| {
                self.error(
                    SyntaxErrorKind::TooLarge,
                    Span::new(start, self.pos),
                    "a string literal longer than 2^30 - 1 code units",
                )
            })
    }

    /// One escape sequence of a string literal, appended to `out` as UTF-16
    /// code units. A `\u` escape appends its code unit (or, above U+FFFF,
    /// its surrogate pair) as is, so two escapes that spell a surrogate pair
    /// give the same code units as the astral character itself, and a lone
    /// surrogate escape stays a lone surrogate (ECMAScript §12.9.4.1 SV;
    /// QuickJS `js_parse_string`).
    fn lex_escape(&mut self, backslash: usize, out: &mut Vec<u16>) -> Result<(), SyntaxError> {
        let Some(c) = self.bump() else {
            return Err(self.error(
                SyntaxErrorKind::UnexpectedEnd,
                Span::new(backslash, self.pos),
                "unterminated string literal",
            ));
        };
        let escape_error = |lexer: &Self, kind, message: &str| {
            lexer.error(kind, Span::new(backslash, lexer.pos), message.to_string())
        };
        match c {
            'n' => out.push(0x0A),
            't' => out.push(0x09),
            'r' => out.push(0x0D),
            'b' => out.push(0x08),
            'f' => out.push(0x0C),
            'v' => out.push(0x0B),
            '0' if !self.peek_char().is_some_and(|c| c.is_ascii_digit()) => out.push(0),
            '0'..='7' => {
                return Err(escape_error(
                    self,
                    SyntaxErrorKind::InvalidToken,
                    "octal escape sequences are not allowed in strict mode",
                ));
            }
            '8' | '9' => {
                return Err(escape_error(
                    self,
                    SyntaxErrorKind::InvalidToken,
                    "\\8 and \\9 escapes are not allowed in strict mode",
                ));
            }
            '\r' => {
                // Line continuation; CRLF is one terminator.
                if self.peek_char() == Some('\n') {
                    self.bump();
                }
            }
            '\n' | '\u{2028}' | '\u{2029}' => {}
            'x' => {
                let value = self.hex_digits(2).ok_or_else(|| {
                    escape_error(
                        self,
                        SyntaxErrorKind::InvalidToken,
                        "invalid hexadecimal escape sequence",
                    )
                })?;
                // Two hex digits: always one code unit below 0x100.
                out.push(value as u16);
            }
            'u' => {
                let value = self.unicode_escape_value().ok_or_else(|| {
                    escape_error(
                        self,
                        SyntaxErrorKind::InvalidToken,
                        "invalid Unicode escape sequence",
                    )
                })?;
                push_code_point(out, value);
            }
            other => push_char(out, other),
        }
        Ok(())
    }

    /// Exactly `n` hex digits.
    fn hex_digits(&mut self, n: usize) -> Option<u32> {
        let text = self.rest().get(..n)?;
        if !text.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        self.pos += n;
        u32::from_str_radix(text, 16).ok()
    }

    /// The part of a `\u` escape after the `u`: `HHHH` or `{H...}` (at most
    /// U+10FFFF). Returns the code point (possibly a surrogate).
    fn unicode_escape_value(&mut self) -> Option<u32> {
        if self.peek_char() == Some('{') {
            let close = self.rest().find('}')?;
            let digits = &self.rest()[1..close];
            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            let trimmed = digits.trim_start_matches('0');
            if trimmed.len() > 6 {
                return None;
            }
            let value = u32::from_str_radix(if trimmed.is_empty() { "0" } else { trimmed }, 16)
                .ok()
                .filter(|v| *v <= 0x10FFFF)?;
            self.pos += close + 1;
            Some(value)
        } else {
            self.hex_digits(4)
        }
    }
}

/// Appends `c` as UTF-16 code units.
fn push_char(out: &mut Vec<u16>, c: char) {
    let mut buf = [0u16; 2];
    out.extend_from_slice(c.encode_utf16(&mut buf));
}

/// Appends a code point of at most U+10FFFF as UTF-16 code units: a
/// surrogate (U+D800..U+DFFF) is appended as that one code unit, so a lone
/// surrogate stays representable; above U+FFFF, the surrogate pair.
fn push_code_point(out: &mut Vec<u16>, value: u32) {
    match char::from_u32(value) {
        Some(c) => push_char(out, c),
        // `unicode_escape_value` returns at most U+10FFFF, so the only
        // values that are not chars are the surrogates, which fit in one
        // code unit.
        None => out.push(value as u16),
    }
}

/// Correctly rounded value of a non-empty digit string in a power-of-two
/// radix (2, 8 or 16).
/// `text`, the source from the start of a number literal, starts with a
/// legacy octal or leading-zero decimal literal (`017`, `08`; also `0_1`,
/// which is invalid in either mode): an early error in strict code.
pub(crate) fn is_leading_zero_literal(text: &str) -> bool {
    text.starts_with('0') && text[1..].starts_with(|c: char| c.is_ascii_digit() || c == '_')
}

/// The strict-mode early error for a legacy octal or leading-zero literal
/// at `start` in `src`, at its leading `0`.
pub(crate) fn leading_zero_in_strict_code(src: &str, start: usize) -> SyntaxError {
    SyntaxError::new(
        src,
        SyntaxErrorKind::InvalidToken,
        Span::new(start, start + 1),
        "legacy octal and leading-zero number literals are not allowed in strict mode",
    )
}

fn radix_value(digits: &str, radix: u32) -> f64 {
    let bits_per_digit = radix.trailing_zeros();
    let digits = digits.trim_start_matches('0');
    // Keep the leading 128 bits exactly; fold every later nonzero bit into a
    // sticky low bit. 128 bits far exceed f64's 53, so the `u128 -> f64`
    // conversion (round to nearest, ties to even) then rounds correctly.
    let mut mantissa: u128 = 0;
    let mut shift: i32 = 0;
    let mut sticky = false;
    for c in digits.chars() {
        let d = c.to_digit(radix).unwrap_or(0) as u128;
        if mantissa.leading_zeros() >= bits_per_digit {
            mantissa = (mantissa << bits_per_digit) | d;
        } else {
            shift += bits_per_digit as i32;
            sticky |= d != 0;
        }
    }
    if sticky {
        mantissa |= 1;
    }
    let value = mantissa as f64;
    if shift == 0 {
        value
    } else {
        // Exact power-of-two scaling; overflow to infinity is correct because
        // the mantissa is already at least 2^120 here.
        value * 2f64.powi(shift)
    }
}
