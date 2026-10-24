//! On-demand tokenizer for the documented JavaScript subset
//! (`docs/grammar.md` §Lexical grammar).
//!
//! The lexer recognizes every ECMAScript punctuator by maximal munch, so that
//! an operator outside the subset (`**`, `>>>=`, `?.`, `=>`, ...) arrives at
//! the parser as one token and is rejected there with a precise diagnostic.
//! A `/` initially lexes as division; the parser requests the regexp goal
//! where an operand is expected. Integer `n` literals retain exact BigInt
//! digits; numeric separators are rejected here.
//!
//! Template literals (clj-quickjs-57) lex as one token per part. The lexer
//! keeps a stack of open substitutions, and the `}` that closes one (the
//! first `}` not matching a `{` opened inside it) starts the template's
//! next part instead of being a punctuator. In this subset a `{` or `}`
//! inside a substitution is counted only when lexed as a punctuator;
//! rescanning a regexp consumes its pattern text without counting braces.
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
    BigInt(crate::value::JsBigInt),
    /// A string literal's cooked value: UTF-16 code units, possibly with
    /// lone surrogates (clj-quickjs-141).
    String(JsString),
    /// One part of a template literal (clj-quickjs-57).
    Template(Box<TemplatePart>),
    Punct(Punct),
    Eof,
    /// A regular expression literal, rescanned at the parser's request.
    RegExp {
        pattern: JsString,
        flags: String,
    },
}

impl TokenKind {
    /// Short human-readable description for diagnostics.
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Identifier(name) => format!("identifier `{name}`"),
            TokenKind::Keyword(k) => format!("keyword `{}`", k.as_str()),
            TokenKind::Number(_) => "number literal".to_string(),
            TokenKind::BigInt(_) => "BigInt literal".to_string(),
            TokenKind::String(_) => "string literal".to_string(),
            TokenKind::Template(part) if part.head => "template literal".to_string(),
            TokenKind::Template(_) => "`}` closing a template substitution".to_string(),
            TokenKind::Punct(p) => format!("`{}`", p.as_str()),
            TokenKind::Eof => "end of input".to_string(),
            TokenKind::RegExp { .. } => "regular expression literal".to_string(),
        }
    }
}

/// One part of a template literal (ECMAScript §12.9.6): a
/// NoSubstitutionTemplate (`head` and `tail`), a TemplateHead (`head`; it
/// ends at the `${` of a substitution), a TemplateMiddle (neither) or a
/// TemplateTail (`tail`; it ends at the closing backtick).
#[derive(Clone, Debug, PartialEq)]
pub struct TemplatePart {
    /// The template value (TV): the text between the delimiters with
    /// escapes decoded as in a string literal and CR and CRLF read as LF,
    /// as UTF-16 code units. An invalid escape (an octal or `\8` `\9`
    /// escape, in either mode, or a malformed `\x` or `\u` escape) gives
    /// its SyntaxError instead: an untagged template rejects it, while a
    /// tagged one (clj-quickjs-58) sees `undefined`.
    pub cooked: Result<JsString, SyntaxError>,
    /// The template raw value (TRV): the text between the delimiters as
    /// written, escapes included, with CR and CRLF read as LF.
    pub raw: JsString,
    /// It starts at a backtick rather than at the `}` closing a
    /// substitution.
    pub head: bool,
    /// It ends at the closing backtick rather than at `${`.
    pub tail: bool,
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
pub(crate) fn is_ident_start(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphabetic() || c == '$' || c == '_'
    } else {
        in_ranges(ID_START, c)
    }
}

/// IdentifierPartChar: `ID_Continue`, `$`, ZWNJ or ZWJ.
pub(crate) fn is_ident_part(c: char) -> bool {
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
    /// Sorted byte positions of runtime source's lone UTF-16 units. U+FFFD
    /// carries them lexically, but only these positions decode to surrogates.
    surrogates: &'a [(usize, u16)],
    pos: usize,
    /// The next token is in an IdentifierName position (a property name:
    /// after `.`, or where the parser asked for one with
    /// [`Lexer::expect_property_name`]), where a reserved word written with
    /// escapes is an ordinary name.
    after_dot: bool,
    /// The code being lexed is strict-mode code, where legacy octal and
    /// leading-zero number literals (clj-quickjs-51) and legacy octal and
    /// `\8` `\9` string escapes (clj-quickjs-258) are early errors. The
    /// parser keeps it in step with its own mode ([`Lexer::set_strict`]).
    strict: bool,
    /// One entry per template substitution open at the current position,
    /// innermost last: how many `{` punctuators inside it are not closed
    /// yet. A `}` while that count is 0 closes the substitution and starts
    /// the template's next part (clj-quickjs-57).
    substitutions: Vec<u32>,
}

impl<'a> Lexer<'a> {
    /// A lexer at the start of `src`, past a hashbang comment (ECMAScript
    /// HashbangComment, QuickJS `skip_shebang`): `#!` at byte 0 up to, not
    /// including, the first line terminator, which still counts for ASI and
    /// line numbers. `#` anywhere else is not a token.
    pub fn new(src: &'a str) -> Lexer<'a> {
        Self::with_surrogates(src, &[])
    }

    pub(crate) fn with_surrogates(src: &'a str, surrogates: &'a [(usize, u16)]) -> Lexer<'a> {
        let pos = if src.starts_with("#!") {
            src.find(is_line_terminator).unwrap_or(src.len())
        } else {
            0
        };
        Lexer {
            src,
            surrogates,
            pos,
            after_dot: false,
            strict: true,
            substitutions: Vec::new(),
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

    /// Append source contents, preserving runtime-only lone code units.
    /// Token recognition always uses the carrier, which cannot form an
    /// identifier, whitespace, delimiter, escape or line terminator.
    fn push_source_char(&self, out: &mut Vec<u16>, at: usize, c: char) {
        if c == '\u{fffd}'
            && let Ok(index) = self.surrogates.binary_search_by_key(&at, |&(pos, _)| pos)
        {
            out.push(self.surrogates[index].1);
        } else {
            push_char(out, c);
        }
    }

    fn source_units(&self, start: usize, end: usize) -> Result<JsString, SyntaxError> {
        let mut out = Vec::new();
        for (offset, c) in self.src[start..end].char_indices() {
            self.push_source_char(&mut out, start + offset, c);
        }
        self.literal_units(out, Span::new(start, end))
    }

    fn literal_units(&self, out: Vec<u16>, span: Span) -> Result<JsString, SyntaxError> {
        JsString::from_code_units(out).ok_or_else(|| {
            self.error(
                SyntaxErrorKind::TooLarge,
                span,
                "a literal longer than 2^30 - 1 code units",
            )
        })
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
            Some('`') => self.lex_template(true)?,
            Some('}') if self.substitutions.last() == Some(&0) => {
                self.substitutions.pop();
                self.lex_template(false)?
            }
            Some(c) => match self.lex_punct() {
                Some(p) => {
                    if let Some(open) = self.substitutions.last_mut() {
                        match p {
                            Punct::LBrace => *open += 1,
                            Punct::RBrace => *open -= 1,
                            _ => {}
                        }
                    }
                    TokenKind::Punct(p)
                }
                None => {
                    let message = match self
                        .surrogates
                        .binary_search_by_key(&start, |&(pos, _)| pos)
                    {
                        Ok(index) => {
                            format!("unexpected surrogate \\u{{{:x}}}", self.surrogates[index].1)
                        }
                        Err(_) => format!("unexpected character {c:?}"),
                    };
                    return Err(self.error_at(start, SyntaxErrorKind::UnknownToken, message));
                }
            },
        };
        self.after_dot = matches!(kind, TokenKind::Punct(Punct::Dot | Punct::OptionalChain));
        Ok(Token {
            kind,
            span: Span::new(start, self.pos),
            newline_before,
        })
    }

    /// Rescan a current `/` or `/=` punctuator with the regexp lexical
    /// goal. No token in its body has been consumed, so template state is
    /// unchanged. The parser validates pattern syntax after rescanning.
    pub(crate) fn rescan_regexp(&mut self, slash: &Token) -> Result<Token, SyntaxError> {
        debug_assert!(matches!(
            slash.kind,
            TokenKind::Punct(Punct::Slash | Punct::SlashAssign)
        ));
        let start = slash.span.start;
        self.pos = start + 1;
        self.after_dot = false;
        let body_start = self.pos;
        let mut in_class = false;
        let body_end = loop {
            let at = self.pos;
            match self.bump() {
                None => {
                    return Err(self.error_at(
                        start,
                        SyntaxErrorKind::UnexpectedEnd,
                        "unterminated regular expression literal",
                    ));
                }
                Some(c) if is_line_terminator(c) => {
                    return Err(self.error_at(
                        start,
                        SyntaxErrorKind::InvalidToken,
                        "line terminator in regular expression literal",
                    ));
                }
                Some('/') if !in_class => break at,
                Some('[') => in_class = true,
                Some(']') => in_class = false,
                Some('\\') => match self.bump() {
                    None => {
                        return Err(self.error_at(
                            start,
                            SyntaxErrorKind::UnexpectedEnd,
                            "unterminated regular expression escape",
                        ));
                    }
                    Some(c) if is_line_terminator(c) => {
                        return Err(self.error_at(
                            start,
                            SyntaxErrorKind::InvalidToken,
                            "line terminator in regular expression literal",
                        ));
                    }
                    Some(_) => {}
                },
                Some(_) => {}
            }
        };
        let flags_start = self.pos;
        while self.peek_char().is_some_and(is_ident_part) {
            self.bump();
        }
        let flags = &self.src[flags_start..self.pos];
        let mut seen = 0u8;
        for flag in flags.chars() {
            let Some(index) = "dgimsuyv".find(flag) else {
                return Err(self.error_at(
                    start,
                    SyntaxErrorKind::Invalid,
                    format!("unknown regular expression flag `{flag}`"),
                ));
            };
            let bit = 1 << index;
            if seen & bit != 0 {
                return Err(self.error_at(
                    start,
                    SyntaxErrorKind::Invalid,
                    format!("duplicate regular expression flag `{flag}`"),
                ));
            }
            seen |= bit;
        }
        if flags.contains('u') && flags.contains('v') {
            return Err(self.error_at(
                start,
                SyntaxErrorKind::Invalid,
                "regular expression flags `u` and `v` are mutually exclusive",
            ));
        }
        // IdentifierPart permits Unicode escapes, but RegExp flags must
        // be the literal flag characters, never escaped spellings.
        if self.peek_char() == Some('\\') {
            return Err(self.error_at(
                start,
                SyntaxErrorKind::Invalid,
                "escaped regular expression flags are not allowed",
            ));
        }
        Ok(Token {
            kind: TokenKind::RegExp {
                pattern: self.source_units(body_start, body_end)?,
                flags: flags.to_string(),
            },
            span: Span::new(start, self.pos),
            newline_before: slash.newline_before,
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
        // A BigInt suffix is valid only after an integer with no leading zero.
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
        if self.peek_char() == Some('n') && bigint_suffix {
            let digits_start = start + if radix == 10 { 0 } else { 2 };
            let digits_end = self.pos;
            self.bump();
            if self
                .peek_char()
                .is_some_and(|c| is_ident_start(c) || c.is_ascii_digit() || c == '\\')
            {
                return Err(self.error_at(
                    self.pos,
                    SyntaxErrorKind::InvalidToken,
                    "identifier or digit starts immediately after BigInt literal",
                ));
            }
            let bigint =
                crate::value::JsBigInt::from_digits(&self.src[digits_start..digits_end], radix)
                    .map_err(|error| {
                        self.error(
                            SyntaxErrorKind::TooLarge,
                            Span::new(start, self.pos),
                            error.to_string(),
                        )
                    })?;
            return Ok(TokenKind::BigInt(bigint));
        }
        match self.peek_char() {
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
                '\\' => self.lex_escape(char_pos, &mut out, false)?,
                c => self.push_source_char(&mut out, char_pos, c),
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
    ///
    /// In a template literal (`template`) a digit escape other than `\0`
    /// not followed by a digit is an error in either mode (ECMAScript
    /// NotEscapeSequence; QuickJS `js_parse_string` with a backtick
    /// separator).
    fn lex_escape(
        &mut self,
        backslash: usize,
        out: &mut Vec<u16>,
        template: bool,
    ) -> Result<(), SyntaxError> {
        let char_pos = self.pos;
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
            '0'..='7' if template => {
                return Err(escape_error(
                    self,
                    SyntaxErrorKind::InvalidToken,
                    "octal escape sequences are not allowed in template literals",
                ));
            }
            '8' | '9' if template => {
                return Err(escape_error(
                    self,
                    SyntaxErrorKind::InvalidToken,
                    "\\8 and \\9 escapes are not allowed in template literals",
                ));
            }
            '0'..='7' if self.strict => {
                return Err(escape_error(
                    self,
                    SyntaxErrorKind::InvalidToken,
                    "octal escape sequences are not allowed in strict mode",
                ));
            }
            '0'..='7' => out.push(self.legacy_octal_escape(c)),
            '8' | '9' if self.strict => {
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
            other => self.push_source_char(out, char_pos, other),
        }
        Ok(())
    }

    /// The rest of a LegacyOctalEscapeSequence of sloppy code whose first
    /// digit `first` was just read (clj-quickjs-258; ECMAScript B.1.2,
    /// QuickJS `js_parse_string` and `lre_parse_escape`): at most two more
    /// octal digits after `0`..`3`, one after `4`..`7`, so the value is at
    /// most 0o377 and `\400` is U+0020 then `0`. Returns that code unit.
    fn legacy_octal_escape(&mut self, first: char) -> u16 {
        let more = if first <= '3' { 2 } else { 1 };
        let mut value = first as u16 - u16::from(b'0');
        for _ in 0..more {
            let Some(digit @ '0'..='7') = self.peek_char() else {
                break;
            };
            self.bump();
            value = value * 8 + (digit as u16 - u16::from(b'0'));
        }
        value
    }

    /// A template literal part (clj-quickjs-57; ECMAScript §12.9.6, QuickJS
    /// `js_parse_template_part`), at its opening delimiter: the backtick of
    /// a NoSubstitutionTemplate or TemplateHead (`head`), or the `}` that
    /// closes a substitution. It ends at an unescaped backtick or at `${`,
    /// which opens a substitution. Line terminators are part of its text.
    fn lex_template(&mut self, head: bool) -> Result<TokenKind, SyntaxError> {
        let start = self.pos;
        self.bump();
        let body_start = self.pos;
        let mut raw = Vec::new();
        let unterminated = |lexer: &Self| {
            lexer.error(
                SyntaxErrorKind::UnexpectedEnd,
                Span::new(start, lexer.pos),
                "unterminated template literal",
            )
        };
        let (tail, body_end) = loop {
            let mut at = self.pos;
            let Some(mut c) = self.bump() else {
                return Err(unterminated(self));
            };
            match c {
                '`' => break (true, at),
                '$' if self.peek_char() == Some('{') => {
                    self.bump();
                    break (false, at);
                }
                // An escaped character never ends the part.
                '\\' => {
                    push_char(&mut raw, c);
                    at = self.pos;
                    c = self.bump().ok_or_else(|| unterminated(self))?;
                }
                _ => {}
            }
            if c == '\r' {
                if self.peek_char() == Some('\n') {
                    self.bump();
                }
                c = '\n';
            }
            self.push_source_char(&mut raw, at, c);
        };
        let cooked = self.template_value(Span::new(body_start, body_end))?;
        if !tail {
            self.substitutions.push(0);
        }
        Ok(TokenKind::Template(Box::new(TemplatePart {
            cooked,
            raw: self.literal_units(raw, Span::new(body_start, body_end))?,
            head,
            tail,
        })))
    }

    /// The template value of the part text at `body`: `Ok(Ok(..))` with the
    /// cooked value, or `Ok(Err(..))` with the error of its first invalid
    /// escape. A value longer than 2^30 - 1 code units is an error either
    /// way.
    fn template_value(&self, body: Span) -> Result<Result<JsString, SyntaxError>, SyntaxError> {
        // A lexer whose source ends with the text, so that no escape can
        // read past it.
        let mut lexer = Lexer {
            src: &self.src[..body.end],
            surrogates: self.surrogates,
            pos: body.start,
            after_dot: false,
            strict: true,
            substitutions: Vec::new(),
        };
        let mut out: Vec<u16> = Vec::new();
        loop {
            let at = lexer.pos;
            match lexer.bump() {
                None => break,
                Some('\\') => {
                    if let Err(e) = lexer.lex_escape(at, &mut out, true) {
                        return Ok(Err(e));
                    }
                }
                Some('\r') => {
                    if lexer.peek_char() == Some('\n') {
                        lexer.bump();
                    }
                    out.push(0x0A);
                }
                Some(c) => lexer.push_source_char(&mut out, at, c),
            }
        }
        JsString::from_code_units(out).map(Ok).ok_or_else(|| {
            self.error(
                SyntaxErrorKind::TooLarge,
                body,
                "a template literal part longer than 2^30 - 1 code units",
            )
        })
    }

    /// Whether a line terminator (possibly inside a comment) comes before
    /// the next token, or `None` if the whitespace and comments before it
    /// do not lex.
    pub(crate) fn newline_before_next(&self) -> Option<bool> {
        self.clone().skip_trivia().ok()
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

/// The strict-mode early error for a legacy octal or `\8` `\9` escape in
/// the string literal token `string` of `src`, which lexed as sloppy code,
/// at its first such escape; `None` if it has none (clj-quickjs-258).
pub(crate) fn legacy_escape_in_strict_code(src: &str, string: Span) -> Option<SyntaxError> {
    if !string.text(src).contains('\\') {
        return None;
    }
    let mut lexer = Lexer {
        src,
        surrogates: &[],
        pos: string.start,
        after_dot: false,
        strict: true,
        substitutions: Vec::new(),
    };
    lexer.next_token().err()
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
